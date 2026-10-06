//! Effects on the GPU (`vita/src/fx.rs`).
//!
//! The pack holds each layer of each effect as a template of vertices and
//! eight rows of constants. A frame gathers the live effects by kind, writes
//! one record per live effect, and for each layer of each kind with any alive
//! issues one instanced draw: the template as one buffer, the records as the
//! other. No particle is stored or stepped.

use pocket_web_wgpu::gpu::Gpu;
use pocket_web_wgpu::wgpu;
use requiem_pack::{self as pack, FxEffect, FxHeader, FxInstance, FxLayer, Pack};
use requiem_sim::fx::{FxList, LIFE, SLOTS};
use requiem_sim::math::*;
use requiem_sim::sim::BOLTS;
use requiem_sim::Sim;

use super::{buffer, module, part, picture, pipeline, sampler_entry, texture_entry, uniform_entry, written, Blend, Depth, Program, SCENE};

/// Live effects one frame can draw.
const CAPACITY: usize = SLOTS + BOLTS;
/// Bytes from one layer's constants to the next in their buffer: what a uniform's offset is a multiple of.
const STEP: u64 = 256;
const ROWS: u64 = 128;

struct Layer {
    program: usize,
    over: bool,
    idx_count: u32,
    vtx: u64,
    vtx_bytes: u64,
    idx: u64,
}

#[derive(Clone, Copy, Default)]
pub struct Stats {
    pub live: u32,
    pub draws: u32,
    pub tris: u32,
}

pub struct Fx {
    /// The pack's section as it is: templates and indices.
    data: wgpu::Buffer,
    instances: wgpu::Buffer,
    group: wgpu::BindGroup,
    /// Per program (particles, ring, ribbon, shell): the one that adds light and the one that covers.
    programs: Vec<[wgpu::RenderPipeline; 2]>,
    effects: Vec<FxEffect>,
    layers: Vec<Layer>,
    /// Per effect kind this frame: first record and count.
    spans: Vec<(u32, u32)>,
    order: Vec<(u8, FxInstance)>,
    records: Vec<FxInstance>,
    live: u32,
    pub bytes: usize,
}

impl Fx {
    pub fn load(gpu: &Gpu, p: &Pack, globals: &wgpu::BindGroupLayout, samples: u32) -> Result<Fx, String> {
        let data = p.section(pack::FXPK)?;
        let head: FxHeader = pack::read(data, 0).ok_or("effects header")?;
        let mut at = core::mem::size_of::<FxHeader>();
        let mut effects = Vec::with_capacity(head.effects as usize);
        for _ in 0..head.effects {
            effects.push(pack::read::<FxEffect>(data, at).ok_or("effect record")?);
            at += core::mem::size_of::<FxEffect>();
        }
        let mut layers = Vec::with_capacity(head.layers as usize);
        let mut constants = vec![0u8; head.layers as usize * STEP as usize];
        for i in 0..head.layers as usize {
            let l: FxLayer = pack::read(data, at).ok_or("effect layer")?;
            at += core::mem::size_of::<FxLayer>();
            if l.vtx_at as usize + l.vtx_count as usize * 12 > data.len() || l.idx_at as usize + l.idx_count as usize * 2 > data.len() || l.program > 3 {
                return Err("the pack's effects section is malformed".into());
            }
            constants[i * STEP as usize..i * STEP as usize + ROWS as usize].copy_from_slice(bytemuck::cast_slice(&l.rows));
            layers.push(Layer { program: l.program as usize, over: l.blend == 1, idx_count: l.idx_count, vtx: l.vtx_at as u64, vtx_bytes: l.vtx_count as u64 * 12, idx: l.idx_at as u64 });
        }
        // The atlas is one byte of brightness per texel.
        let n = (head.atlas * head.atlas) as usize;
        let texels = data.get(head.atlas_at as usize..head.atlas_at as usize + n).ok_or("effects atlas")?;
        let view = picture(gpu, "effects", wgpu::TextureFormat::R8Unorm, head.atlas, head.atlas, &[texels.to_vec()]);
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor { label: Some("effects"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let rows = buffer(gpu, "effect layers", &constants, wgpu::BufferUsages::UNIFORM);
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("effects"), entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX, ROWS, true), texture_entry(1), sampler_entry(2)] });
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effects"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: part(&rows, ROWS) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) }],
        });
        // A template buffer and one record per live effect.
        let module = module(gpu, "effects", include_str!("../shaders/fx.wgsl"));
        let buffers = [
            wgpu::VertexBufferLayout { array_stride: 12, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Snorm8x4, 1 => Snorm8x4, 2 => Snorm8x4] },
            wgpu::VertexBufferLayout { array_stride: 32, step_mode: wgpu::VertexStepMode::Instance, attributes: &wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4] },
        ];
        let programs = ["particles", "ring", "ribbon", "shell"]
            .into_iter()
            .map(|vertex| [Blend::Additive, Blend::Premultiplied].map(|blend| pipeline(gpu, &Program { label: vertex, module: &module, vertex, fragment: "glow", groups: &[globals, &layout], buffers: &buffers, format: SCENE, samples, depth: Depth::Tested, blend })))
            .collect();
        Ok(Fx {
            data: buffer(gpu, "effects", data, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::INDEX),
            instances: written(gpu, "live effects", CAPACITY * core::mem::size_of::<FxInstance>(), wgpu::BufferUsages::VERTEX),
            group,
            programs,
            spans: vec![(0, 0); effects.len()],
            effects,
            layers,
            order: Vec::with_capacity(CAPACITY),
            records: Vec::with_capacity(CAPACITY),
            live: 0,
            bytes: data.len(),
        })
    }

    /// Gathers the live effects by kind and writes their records.
    pub fn choose(&mut self, gpu: &Gpu, sim: &Sim) {
        self.order.clear();
        for f in &sim.fx.items {
            if !FxList::alive(f, sim.tick) || f.kind as usize >= self.effects.len() {
                continue;
            }
            let age = sim.tick.wrapping_sub(f.t0) as f32 / LIFE[f.kind as usize] as f32;
            self.order.push((f.kind, FxInstance { pos: [f.pos.x, f.pos.y, f.pos.z], age, dir: [f.dir.x, f.dir.y, f.dir.z], a: f.a }));
        }
        // A bolt in flight is the effect after the simulation's last kind, held at one age, pointing back along its path.
        let bolt = (self.effects.len() - 1) as u8;
        for b in sim.bolts.iter().filter(|b| b.alive != 0) {
            let d = -b.vel.norm_or(V3::UP);
            self.order.push((bolt, FxInstance { pos: [b.pos.x, b.pos.y, b.pos.z], age: 0.3, dir: [d.x, d.y, d.z], a: 1.0 }));
        }
        self.order.truncate(CAPACITY);
        self.live = self.order.len() as u32;
        self.order.sort_by_key(|k| k.0);
        for s in self.spans.iter_mut() {
            *s = (0, 0);
        }
        self.records.clear();
        for (k, (kind, record)) in self.order.iter().enumerate() {
            self.records.push(*record);
            let s = &mut self.spans[*kind as usize];
            if s.1 == 0 {
                s.0 = k as u32;
            }
            s.1 += 1;
        }
        if !self.records.is_empty() {
            gpu.queue.write_buffer(&self.instances, 0, pack::slice_bytes(&self.records));
        }
    }

    /// Draws every live effect: the layers that cover first, then the ones that add light.
    pub fn draw(&self, pass: &mut wgpu::RenderPass) -> Stats {
        let mut stats = Stats { live: self.live, ..Default::default() };
        if self.live == 0 {
            return stats;
        }
        pass.set_vertex_buffer(1, self.instances.slice(..));
        for over in [true, false] {
            let mut bound = usize::MAX;
            for (kind, e) in self.effects.iter().enumerate() {
                let (first, count) = self.spans[kind];
                if count == 0 {
                    continue;
                }
                for index in e.first as usize..(e.first + e.count) as usize {
                    let l = &self.layers[index];
                    if l.over != over {
                        continue;
                    }
                    if bound != l.program {
                        pass.set_pipeline(&self.programs[l.program][over as usize]);
                        bound = l.program;
                    }
                    pass.set_bind_group(1, &self.group, &[(index as u64 * STEP) as u32]);
                    pass.set_vertex_buffer(0, self.data.slice(l.vtx..l.vtx + l.vtx_bytes));
                    pass.set_index_buffer(self.data.slice(l.idx..l.idx + l.idx_count as u64 * 2), wgpu::IndexFormat::Uint16);
                    pass.draw_indexed(0..l.idx_count, 0, first..first + count);
                    stats.draws += 1;
                    stats.tris += l.idx_count / 3 * count;
                }
            }
        }
        stats
    }
}
