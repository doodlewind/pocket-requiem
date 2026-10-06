//! The army on the GPU (`vita/src/crowd.rs`).
//!
//! The pack holds, for each kind of knight and each level of detail, every
//! vertex placed at every stored frame. A frame of the game asks the
//! simulation which knights are in view and which two stored frames each
//! shows, sorts them by (mesh, first frame, second frame), writes one record
//! per knight and issues one instanced draw per run of equal keys. Nothing is
//! skinned and no uniform changes between draws.

use pocket_web_wgpu::gpu::Gpu;
use pocket_web_wgpu::wgpu;
use requiem_pack::{self as pack, CrowdHeader, CrowdInstance, CrowdMesh, CrowdVertex, Pack};
use requiem_sim::crowd::Draw;
use requiem_sim::math::*;
use requiem_sim::Sim;

use super::{buffer, module, pipeline, write_part, written, Blend, Depth, Program, Scene, SCENE};
use crate::pack::Ranges;

/// Knights one frame can draw.
pub const CAPACITY: usize = 3072;

struct Mesh {
    vtx_count: u64,
    idx_count: u32,
    color: u64,
    idx: u64,
    frames: u64,
}

#[derive(Clone, Copy, Default)]
pub struct Stats {
    pub shown: u32,
    pub draws: u32,
    pub tris: u32,
    pub by_lod: [u32; 5],
    /// How far the budget pulled the hand-over distances in this frame (1: not at all).
    pub pulled: f32,
}

pub struct Crowd {
    /// The pack's section as it is: frames, colours and indices.
    data: wgpu::Buffer,
    instances: wgpu::Buffer,
    near: wgpu::RenderPipeline,
    far_program: wgpu::RenderPipeline,
    meshes: Vec<Mesh>,
    /// Where the section starts in the pack, and its size.
    at: (u64, u64),
    frames: u64,
    /// Per mesh, whether its frames, colours and indices have arrived, and the level drawn in its place:
    /// its own, or the next coarser one that is here.
    here: Vec<bool>,
    shown: Vec<usize>,
    lods: usize,
    /// Squared distance at which each level of detail hands over to the next.
    reach2: Vec<f32>,
    pub far: f32,
    /// Triangles of knights a frame may draw.
    pub budget: usize,
    pub scale: f32,
    draws: Vec<Draw>,
    order: Vec<(u32, u32)>,
    records: Vec<CrowdInstance>,
    stats: Stats,
    pub bytes: usize,
}

impl Crowd {
    pub fn load(gpu: &Gpu, p: &Pack, have: &Ranges, scene: &Scene, globals: &wgpu::BindGroupLayout, samples: u32) -> Result<Crowd, String> {
        let data = p.section(pack::CRWD)?;
        let head: CrowdHeader = pack::read(data, 0).ok_or("crowd header")?;
        let count = (head.kinds * head.lods) as usize;
        let reach = &scene.crowd_reach;
        if reach.len() != head.lods as usize || head.frames > 127 {
            return Err(format!("the pack's crowd has {} levels and {} frames; the profile gives {} distances", head.lods, head.frames, reach.len()));
        }
        let mut meshes = Vec::with_capacity(count);
        for i in 0..count {
            let m: CrowdMesh = pack::read(data, core::mem::size_of::<CrowdHeader>() + i * core::mem::size_of::<CrowdMesh>()).ok_or("crowd mesh")?;
            let end = m.frames_at as usize + head.frames as usize * m.vtx_count as usize * core::mem::size_of::<CrowdVertex>();
            if end > data.len() || (m.kind * head.lods + m.lod) as usize != i {
                return Err("the pack's crowd section is malformed".into());
            }
            meshes.push(Mesh { vtx_count: m.vtx_count as u64, idx_count: m.idx_count, color: m.color_at as u64, idx: m.idx_at as u64, frames: m.frames_at as u64 });
        }
        let buffer = buffer(gpu, "crowd", data, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST);
        let at = p.range(pack::CRWD).map(|(at, size)| (at as u64, size as u64)).ok_or("the pack's table")?;
        let instances = written(gpu, "knights", CAPACITY * core::mem::size_of::<CrowdInstance>(), wgpu::BufferUsages::VERTEX);

        // Two buffers of stored frames, one of colours, one record per knight.
        let module = module(gpu, "crowd", include_str!("../shaders/crowd.wgsl"));
        let buffers = [
            wgpu::VertexBufferLayout { array_stride: 12, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Sint16x4, 1 => Snorm8x4] },
            wgpu::VertexBufferLayout { array_stride: 12, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![2 => Sint16x4, 3 => Snorm8x4] },
            wgpu::VertexBufferLayout { array_stride: 4, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![4 => Unorm8x4] },
            wgpu::VertexBufferLayout { array_stride: 20, step_mode: wgpu::VertexStepMode::Instance, attributes: &wgpu::vertex_attr_array![5 => Float32x3, 6 => Sint16x2, 7 => Unorm8x4] },
        ];
        let program = |label, vertex| pipeline(gpu, &Program { label, module: &module, vertex, fragment: "tint", groups: &[globals], buffers: &buffers, format: SCENE, samples, depth: Depth::Solid, blend: Blend::Opaque });
        let mut crowd = Crowd {
            at,
            frames: head.frames as u64,
            here: vec![false; count],
            shown: (0..count).map(|i| i % head.lods as usize).collect(),
            data: buffer,
            instances,
            near: program("crowd", "near"),
            far_program: program("crowd far", "far"),
            meshes,
            lods: head.lods as usize,
            reach2: reach.iter().map(|r| r * r).collect(),
            far: *reach.last().unwrap_or(&300.0),
            budget: scene.crowd_budget,
            scale: head.scale,
            draws: Vec::with_capacity(CAPACITY),
            order: Vec::with_capacity(CAPACITY),
            records: Vec::with_capacity(CAPACITY),
            stats: Stats::default(),
            bytes: data.len(),
        };
        crowd.arrive(gpu, have, None);
        Ok(crowd)
    }

    /// Takes a read of the pack that arrived after the start (its offset and its bytes) into the buffer,
    /// and marks the meshes whose frames, colours and indices are all in `have`. A level that is not here
    /// is drawn as the next coarser one that is.
    pub fn arrive(&mut self, gpu: &Gpu, have: &Ranges, read: Option<(u64, &[u8])>) {
        if let Some((offset, bytes)) = read {
            write_part(gpu, &self.data, self.at.0, self.at.1, offset, bytes);
        }
        let stride = core::mem::size_of::<CrowdVertex>() as u64;
        for (m, here) in self.meshes.iter().zip(self.here.iter_mut()).filter(|(_, here)| !**here) {
            let part = |from: u64, size: u64| have.cover(self.at.0 + from, self.at.0 + from + size);
            *here = part(m.frames, self.frames * m.vtx_count * stride) && part(m.color, m.vtx_count * 4) && part(m.idx, m.idx_count as u64 * 2);
        }
        for kind in 0..self.meshes.len() / self.lods {
            for lod in (0..self.lods).rev() {
                let i = kind * self.lods + lod;
                self.shown[i] = if self.here[i] || lod + 1 == self.lods { lod } else { self.shown[i + 1] };
            }
        }
    }

    /// Meshes that have not arrived yet.
    pub fn waiting(&self) -> usize {
        self.here.iter().filter(|here| !**here).count()
    }

    /// Gathers every knight in view, orders them by what they show and writes their records. `scale` pulls
    /// the hand-over distances in (below 1) to shed load.
    pub fn choose(&mut self, gpu: &Gpu, sim: &Sim, planes: &[[f32; 4]; 6], eye: V3, scale: f32) {
        let mut stats = Stats::default();
        sim.crowd.draw(&sim.field, sim.tick, planes, eye, self.far * scale, &mut self.draws);
        self.order.clear();
        // The triangle budget: pull the hand-over distances in (never the last, where a knight stops being drawn)
        // until the knights in view fit. A press of knights round the eye is then drawn a level coarser, not late.
        // (a level whose frames are still on their way is drawn as the next coarser one that is here)
        let level = |kind: usize, d2: f32, k2: f32| {
            let mut lod = self.lods - 1;
            for (l, r) in self.reach2[..self.lods - 1].iter().enumerate() {
                if d2 < r * k2 {
                    lod = l;
                    break;
                }
            }
            self.shown[kind * self.lods + lod]
        };
        let mut k2 = scale * scale;
        for _ in 0..7 {
            let tris: usize = self.draws.iter().take(CAPACITY).map(|d| self.meshes[(d.kind as usize).min(2) * self.lods + level((d.kind as usize).min(2), d.dist2, k2)].idx_count as usize / 3).sum();
            if tris <= self.budget {
                break;
            }
            k2 *= 0.62;
        }
        stats.pulled = sqrt(k2) / scale;
        for (i, d) in self.draws.iter().enumerate().take(CAPACITY) {
            let lod = level((d.kind as usize).min(2), d.dist2, k2);
            stats.by_lod[lod.min(4)] += 1;
            // Level first, so the draws of one program are together.
            self.order.push(((lod as u32) << 16 | (d.kind as u32).min(2) << 14 | (d.a as u32) << 7 | d.b as u32, i as u32));
        }
        self.order.sort_unstable_by_key(|k| k.0);
        self.records.clear();
        for &(_, i) in &self.order {
            let d = &self.draws[i as usize];
            self.records.push(CrowdInstance {
                pos: [d.pos.x, d.pos.y, d.pos.z],
                turn: [(sin(d.yaw) * 32767.0) as i16, (cos(d.yaw) * 32767.0) as i16],
                blend: (d.blend * 255.0) as u8,
                flash: (d.flash * 255.0) as u8,
                grow: ((d.scale - 1.0) * 255.0) as u8,
                pad: 0,
            });
        }
        if !self.records.is_empty() {
            gpu.queue.write_buffer(&self.instances, 0, pack::slice_bytes(&self.records));
        }
        stats.shown = self.order.len() as u32;
        self.stats = stats;
    }

    /// One instanced draw per run of knights that show the same mesh and the same two frames. Levels of
    /// detail from `far_from` on are drawn with the far program.
    pub fn draw(&mut self, pass: &mut wgpu::RenderPass, far_from: usize) -> Stats {
        let n = self.order.len();
        if n == 0 {
            return self.stats;
        }
        let stride = core::mem::size_of::<CrowdVertex>() as u64;
        pass.set_vertex_buffer(3, self.instances.slice(..));
        let mut k = 0;
        let mut bound: Option<bool> = None;
        while k < n {
            let key = self.order[k].0;
            let mut e = k + 1;
            while e < n && self.order[e].0 == key {
                e += 1;
            }
            let lod = (key >> 16) as usize;
            let far = lod >= far_from;
            if bound != Some(far) {
                pass.set_pipeline(if far { &self.far_program } else { &self.near });
                bound = Some(far);
            }
            let m = &self.meshes[((key >> 14) & 3) as usize * self.lods + lod];
            let (a, b) = (((key >> 7) & 127) as u64, (key & 127) as u64);
            let frame = m.vtx_count * stride;
            pass.set_vertex_buffer(0, self.data.slice(m.frames + a * frame..m.frames + (a + 1) * frame));
            pass.set_vertex_buffer(1, self.data.slice(m.frames + b * frame..m.frames + (b + 1) * frame));
            pass.set_vertex_buffer(2, self.data.slice(m.color..m.color + m.vtx_count * 4));
            pass.set_index_buffer(self.data.slice(m.idx..m.idx + m.idx_count as u64 * 2), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..m.idx_count, 0, k as u32..e as u32);
            self.stats.draws += 1;
            self.stats.tris += m.idx_count / 3 * (e - k) as u32;
            k = e;
        }
        self.stats
    }
}
