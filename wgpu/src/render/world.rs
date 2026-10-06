//! The static world on the GPU: every mesh of the pack in one vertex buffer
//! and one index buffer, and the per-frame choice of what to draw
//! (`vita/src/world.rs`).
//!
//! A frame walks the 256 m super-cells. One beyond the middle distance draws
//! its far mesh; a closer one draws each of its 64 m cells, detailed inside
//! the near distance and simple outside it. Everything is culled against the
//! frustum by its bounds first. A mesh's bounds are one record of a buffer
//! read at the rate of instances: a draw names its mesh as its instance.

use std::collections::BTreeMap;

use pocket_web_wgpu::gpu::Gpu;
use pocket_web_wgpu::wgpu;
use requiem_pack::{self as pack, mesh_kind, MeshRec, Pack, TexHeader};
use requiem_sim::math::V3;

use super::{buffer, module, picture, pipeline, sampler_entry, texture_entry, Blend, Depth, Program, Scene, SCENE};
use crate::mat;

#[derive(Clone, Copy, Default)]
struct Range {
    first: u32,
    count: u32,
}

struct Cell {
    min: [f32; 3],
    max: [f32; 3],
    near: Range,
    mid: Range,
}

struct Super {
    min: [f32; 3],
    max: [f32; 3],
    far: Range,
    cells: Range,
}

#[derive(Clone, Copy, Default)]
pub struct Stats {
    pub draws: u32,
    pub tris: u32,
    pub near: u32,
    pub mid: u32,
    pub far: u32,
    /// Meshes drawn with a spell's light on them.
    pub lit: u32,
}

pub struct World {
    vtx: wgpu::Buffer,
    idx: wgpu::Buffer,
    bounds: wgpu::Buffer,
    atlas: wgpu::BindGroup,
    plain: wgpu::RenderPipeline,
    lit: wgpu::RenderPipeline,
    recs: Vec<MeshRec>,
    /// Mesh indices, grouped: every range above points in here.
    lists: Vec<u32>,
    cells: Vec<Cell>,
    supers: Vec<Super>,
    backdrop: Range,
    /// This frame's meshes: those drawn as baked, and those a spell's light reaches.
    chosen: Vec<u32>,
    chosen_lit: Vec<u32>,
    stats: Stats,
    pub bytes: usize,
}

fn grow(min: &mut [f32; 3], max: &mut [f32; 3], r: &MeshRec) {
    for a in 0..3 {
        min[a] = min[a].min(r.min[a]);
        max[a] = max[a].max(r.max[a]);
    }
}

/// One level of BC1 blocks, in row order, as RGBA.
fn bc1(blocks: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![255u8; w * h * 4];
    let colour = |c: u16| [((c >> 11) & 31) as u32 * 255 / 31, ((c >> 5) & 63) as u32 * 255 / 63, (c & 31) as u32 * 255 / 31];
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    for by in 0..bh {
        for bx in 0..bw {
            let b = &blocks[(by * bw + bx) * 8..(by * bw + bx) * 8 + 8];
            let (c0, c1) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
            let (a, z) = (colour(c0), colour(c1));
            let mix = |p: u32, q: u32, c: usize| ((a[c] * p + z[c] * q) / (p + q)) as u8;
            let table: [[u8; 3]; 4] = if c0 > c1 {
                [[a[0] as u8, a[1] as u8, a[2] as u8], [z[0] as u8, z[1] as u8, z[2] as u8], [mix(2, 1, 0), mix(2, 1, 1), mix(2, 1, 2)], [mix(1, 2, 0), mix(1, 2, 1), mix(1, 2, 2)]]
            } else {
                [[a[0] as u8, a[1] as u8, a[2] as u8], [z[0] as u8, z[1] as u8, z[2] as u8], [mix(1, 1, 0), mix(1, 1, 1), mix(1, 1, 2)], [0, 0, 0]]
            };
            let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
            for i in 0..16 {
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                if x < w && y < h {
                    out[(y * w + x) * 4..(y * w + x) * 4 + 3].copy_from_slice(&table[((bits >> (i * 2)) & 3) as usize]);
                }
            }
        }
    }
    out
}

impl World {
    pub fn load(gpu: &Gpu, p: &Pack, scene: &Scene, globals: &wgpu::BindGroupLayout, samples: u32) -> Result<World, String> {
        let recs = p.meshes()?;
        let (vtx_src, idx_src) = (p.section(pack::VTX0)?, p.section(pack::IDX0)?);
        let vtx = buffer(gpu, "world vertices", vtx_src, wgpu::BufferUsages::VERTEX);
        let idx = buffer(gpu, "world indices", idx_src, wgpu::BufferUsages::INDEX);
        let records: Vec<f32> = recs.iter().flat_map(|r| [r.min[0], r.min[1], r.min[2], r.max[0] - r.min[0], r.max[1] - r.min[1], r.max[2] - r.min[2]]).collect();
        let bounds = buffer(gpu, "world bounds", bytemuck::cast_slice(&records), wgpu::BufferUsages::VERTEX);

        // Group meshes: by cell for near and middle, by super-cell for far.
        let per = (scene.super_cell / scene.cell).round().max(1.0) as i32;
        let mut by_cell: BTreeMap<(i32, i32), (Vec<u32>, Vec<u32>)> = BTreeMap::new();
        let mut by_super: BTreeMap<(i32, i32), Vec<u32>> = BTreeMap::new();
        let mut back = Vec::new();
        for (i, r) in recs.iter().enumerate() {
            match r.kind {
                mesh_kind::NEAR => by_cell.entry((r.cz, r.cx)).or_default().0.push(i as u32),
                mesh_kind::MID => by_cell.entry((r.cz, r.cx)).or_default().1.push(i as u32),
                mesh_kind::FAR => by_super.entry((r.cz, r.cx)).or_default().push(i as u32),
                _ => back.push(i as u32),
            }
        }
        let mut lists = Vec::new();
        let mut push = |v: &[u32]| {
            let r = Range { first: lists.len() as u32, count: v.len() as u32 };
            lists.extend_from_slice(v);
            r
        };
        let backdrop = push(&back);
        let empty = || ([f32::MAX; 3], [f32::MIN; 3]);
        let mut super_cells: BTreeMap<(i32, i32), Vec<Cell>> = BTreeMap::new();
        for ((cz, cx), (near, mid)) in &by_cell {
            let (mut min, mut max) = empty();
            for &i in near.iter().chain(mid) {
                grow(&mut min, &mut max, &recs[i as usize]);
            }
            let near_r = push(near);
            // A cell with one level of detail draws it at every distance.
            let mid_r = if mid.is_empty() { near_r } else { push(mid) };
            super_cells.entry((cz.div_euclid(per), cx.div_euclid(per))).or_default().push(Cell { min, max, near: near_r, mid: mid_r });
        }
        let mut keys: Vec<(i32, i32)> = super_cells.keys().chain(by_super.keys()).copied().collect();
        keys.sort();
        keys.dedup();
        let mut cells = Vec::new();
        let mut supers = Vec::new();
        for k in keys {
            let (mut min, mut max) = empty();
            let far = match by_super.get(&k) {
                Some(v) => {
                    for &i in v {
                        grow(&mut min, &mut max, &recs[i as usize]);
                    }
                    push(v)
                }
                None => Range::default(),
            };
            let first = cells.len() as u32;
            for c in super_cells.remove(&k).unwrap_or_default() {
                for a in 0..3 {
                    min[a] = min[a].min(c.min[a]);
                    max[a] = max[a].max(c.max[a]);
                }
                cells.push(c);
            }
            supers.push(Super { min, max, far, cells: Range { first, count: cells.len() as u32 - first } });
        }

        // The atlas: BC1 in the pack, a format every WebGPU device samples here.
        let tex = p.section(pack::TEX0)?;
        let head: TexHeader = pack::read(tex, 0).ok_or("atlas header")?;
        if head.format != pack::tex_format::BC1 {
            return Err("the pack's atlas is not the PS Vita's".into());
        }
        let mut at = core::mem::size_of::<TexHeader>();
        let mut levels = Vec::new();
        for level in 0..head.mips {
            let (w, h) = ((head.width >> level).max(1), (head.height >> level).max(1));
            let size = pack::tex_format::level_bytes(head.format, w, h);
            levels.push(bc1(tex.get(at..at + size).ok_or("the atlas is cut short")?, w as usize, h as usize));
            at += size;
        }
        let view = picture(gpu, "atlas", wgpu::TextureFormat::Rgba8Unorm, head.width, head.height, &levels);
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("atlas"), entries: &[texture_entry(0), sampler_entry(1)] });
        let atlas = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) }],
        });

        // Sixteen-bit attributes are read as whole numbers and scaled in the program.
        let module = module(gpu, "world", include_str!("../shaders/world.wgsl"));
        let buffers = [
            wgpu::VertexBufferLayout { array_stride: 16, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Uint16x4, 1 => Sint16x2, 2 => Unorm8x4] },
            wgpu::VertexBufferLayout { array_stride: 24, step_mode: wgpu::VertexStepMode::Instance, attributes: &wgpu::vertex_attr_array![3 => Float32x3, 4 => Float32x3] },
        ];
        let program = |label, vertex| pipeline(gpu, &Program { label, module: &module, vertex, fragment: "shade", groups: &[globals, &layout], buffers: &buffers, format: SCENE, samples, depth: Depth::Solid, blend: Blend::Opaque });
        let (plain, lit) = (program("world", "plain"), program("world lit", "lit"));
        let bytes = vtx_src.len() + idx_src.len() + levels.iter().map(Vec::len).sum::<usize>();
        Ok(World { vtx, idx, bounds, atlas, plain, lit, recs, lists, cells, supers, backdrop, chosen: Vec::with_capacity(512), chosen_lit: Vec::with_capacity(64), stats: Stats::default(), bytes })
    }

    fn take(&mut self, planes: &[[f32; 4]; 6], r: Range, test: bool, lights: &[(V3, f32)]) {
        for k in r.first as usize..(r.first + r.count) as usize {
            let i = self.lists[k];
            let m = &self.recs[i as usize];
            if test && !mat::visible(planes, &m.min, &m.max) {
                continue;
            }
            // Within a spell's light: it draws later, lit.
            if lights.iter().any(|(p, r)| mat::box_distance(*p, &m.min, &m.max) < *r) {
                self.chosen_lit.push(i);
            } else {
                self.chosen.push(i);
            }
        }
    }

    /// Chooses the meshes of a frame from `eye`. `spells` is the table of lights the lit programs take.
    pub fn choose(&mut self, planes: &[[f32; 4]; 6], eye: V3, lod_near: f32, lod_mid: f32, spells: &[f32; 32]) {
        self.stats = Stats::default();
        self.chosen.clear();
        self.chosen_lit.clear();
        // The lights that are on, as centres and radii.
        let mut lights = [(V3::ZERO, 0.0f32); 4];
        let mut on = 0;
        for k in 0..4 {
            let t = &spells[k * 8..k * 8 + 8];
            if t[4] + t[5] + t[6] > 0.01 {
                lights[on] = (V3 { x: t[0], y: t[1], z: t[2] }, 1.0 / requiem_sim::math::sqrt(t[3].max(1e-6)));
                on += 1;
            }
        }
        let lights = &lights[..on];
        self.take(planes, self.backdrop, false, &[]);
        for si in 0..self.supers.len() {
            let (s_far, s_cells, s_min, s_max) = (self.supers[si].far, self.supers[si].cells, self.supers[si].min, self.supers[si].max);
            if !mat::visible(planes, &s_min, &s_max) {
                continue;
            }
            let before = self.chosen.len() + self.chosen_lit.len();
            if s_far.count > 0 && mat::box_distance(eye, &s_min, &s_max) > lod_mid {
                self.take(planes, s_far, true, &[]);
                self.stats.far += (self.chosen.len() + self.chosen_lit.len() - before) as u32;
                continue;
            }
            for ci in s_cells.first as usize..(s_cells.first + s_cells.count) as usize {
                let (c_min, c_max, c_near, c_mid) = (self.cells[ci].min, self.cells[ci].max, self.cells[ci].near, self.cells[ci].mid);
                if !mat::visible(planes, &c_min, &c_max) {
                    continue;
                }
                let before = self.chosen.len() + self.chosen_lit.len();
                if mat::box_distance(eye, &c_min, &c_max) < lod_near {
                    self.take(planes, c_near, false, lights);
                    self.stats.near += (self.chosen.len() + self.chosen_lit.len() - before) as u32;
                } else {
                    self.take(planes, c_mid, false, lights);
                    self.stats.mid += (self.chosen.len() + self.chosen_lit.len() - before) as u32;
                }
            }
        }
        self.stats.lit = self.chosen_lit.len() as u32;
    }

    /// Draws what `choose` chose: the baked meshes, then the ones in a spell's light.
    pub fn draw(&mut self, pass: &mut wgpu::RenderPass) -> Stats {
        pass.set_bind_group(1, &self.atlas, &[]);
        pass.set_vertex_buffer(0, self.vtx.slice(..));
        pass.set_vertex_buffer(1, self.bounds.slice(..));
        pass.set_index_buffer(self.idx.slice(..), wgpu::IndexFormat::Uint16);
        for (program, list) in [(&self.plain, &self.chosen), (&self.lit, &self.chosen_lit)] {
            if list.is_empty() {
                continue;
            }
            pass.set_pipeline(program);
            for &i in list {
                let m = &self.recs[i as usize];
                pass.draw_indexed(m.idx_first..m.idx_first + m.idx_count, m.vtx_first as i32, i..i + 1);
                self.stats.draws += 1;
                self.stats.tris += m.idx_count / 3;
            }
        }
        self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::bc1;

    #[test]
    fn a_block_of_four_colours_and_one_of_three() {
        // Red and blue ends, the first greater: the four texels of the first row are the ends and the two thirds between.
        let four = [0x00, 0xf8, 0x1f, 0x00, 0b1110_0100, 0, 0, 0];
        let out = bc1(&four, 4, 4);
        assert_eq!(&out[0..16], &[255, 0, 0, 255, 0, 0, 255, 255, 170, 0, 85, 255, 85, 0, 170, 255]);
        // The first not greater: the third colour is their mean and the fourth is black.
        let three = [0x1f, 0x00, 0x00, 0xf8, 0b1110_0100, 0, 0, 0];
        let out = bc1(&three, 4, 4);
        assert_eq!(&out[0..16], &[0, 0, 255, 255, 255, 0, 0, 255, 127, 0, 127, 255, 0, 0, 0, 255]);
    }

    #[test]
    fn a_level_smaller_than_a_block_is_cut_from_one() {
        let block = [0xff, 0xff, 0x00, 0x00, 0, 0, 0, 0];
        assert_eq!(bc1(&block, 2, 2), vec![255; 16]);
    }
}
