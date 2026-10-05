//! The static world on the GPU: every mesh of the pack in one block of
//! GPU-mapped memory, and the per-frame choice of what to draw.
//!
//! A frame walks the 256 m super-cells. One beyond the middle distance draws
//! its far mesh; a closer one draws each of its 64 m cells, detailed inside
//! the near distance and simple outside it. Everything is culled against the
//! frustum by its bounds first.

use requiem_pack::{self as pack, mesh_kind, MeshRec, Pack, TexHeader};
use requiem_sim::math::V3;
use pocket_vita_gxm::mem::{Arena, Block, Kind};
use pocket_vita_gxm::texture::{Format, Texture, Uploader, Wrap};
use std::collections::BTreeMap;
use vita2d_sys as g;

use crate::gpu::{self, Program};
use crate::mat::{self, Mat4};

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
    _block: Block,
    vtx: *const u8,
    idx: *const u16,
    recs: Vec<MeshRec>,
    /// Mesh indices, grouped: every range above points in here.
    lists: Vec<u32>,
    cells: Vec<Cell>,
    supers: Vec<Super>,
    backdrop: Range,
    pub atlas: Texture,
    pub bytes: usize,
    /// Meshes of this frame that a spell's light reaches: drawn after the rest, with the lit program.
    lit: Vec<u32>,
}

/// The program for meshes a spell lights, and where its uniforms go.
pub struct LitWorld<'a> {
    pub prog: &'a Program,
    pub bounds: *const g::SceGxmProgramParameter,
    pub cast: *const g::SceGxmProgramParameter,
    /// Per light: its place and radius, then its colour (as the lit programs take them, with 1 / radius² in w).
    pub table: &'a [f32; 32],
}

fn grow(min: &mut [f32; 3], max: &mut [f32; 3], r: &MeshRec) {
    for a in 0..3 {
        min[a] = min[a].min(r.min[a]);
        max[a] = max[a].max(r.max[a]);
    }
}

impl World {
    /// # Safety
    /// GXM is initialized; `vram` outlives the world.
    pub unsafe fn load(p: &Pack, vram: &mut Arena, super_per_cell: i32) -> Result<World, String> {
        let recs = p.meshes()?;
        let vtx_src = p.section(pack::VTX0)?;
        let idx_src = p.section(pack::IDX0)?;
        let idx_at = (vtx_src.len() + 15) & !15;
        let mut block = Block::with_access(Kind::Main, idx_at + idx_src.len(), false)?;
        let base = block.alloc(idx_at + idx_src.len(), 16).ok_or("world geometry block")?;
        core::ptr::copy_nonoverlapping(vtx_src.as_ptr(), base, vtx_src.len());
        core::ptr::copy_nonoverlapping(idx_src.as_ptr(), base.add(idx_at), idx_src.len());

        // Group meshes: by cell for near and middle, by super-cell for far.
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
            super_cells.entry((cz.div_euclid(super_per_cell), cx.div_euclid(super_per_cell))).or_default().push(Cell { min, max, near: near_r, mid: mid_r });
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

        let tex = p.section(pack::TEX0)?;
        let head: TexHeader = pack::read(tex, 0).ok_or("atlas header")?;
        let mut up = Uploader::new(4 * 1024 * 1024)?;
        let mut atlas = up.texture(vram, Format::Bc1, head.width, head.height, head.mips, &tex[core::mem::size_of::<TexHeader>()..])?;
        up.flush();
        up.free();
        atlas.set_wrap(Wrap::Repeat, Wrap::Clamp);
        atlas.set_filter(true, true);

        Ok(World { vtx: base, idx: base.add(idx_at).cast(), bytes: block.size(), _block: block, recs, lists, cells, supers, backdrop, atlas, lit: Vec::with_capacity(64) })
    }

    #[inline]
    unsafe fn draw_range(&mut self, ctx: *mut g::SceGxmContext, prog: &Program, vp: &Mat4, planes: &[[f32; 4]; 6], r: Range, stats: &mut Stats, test: bool, lights: &[(V3, f32)]) {
        for k in r.first as usize..(r.first + r.count) as usize {
            let i = self.lists[k];
            let m = &self.recs[i as usize];
            if test && !mat::visible(planes, &m.min, &m.max) {
                continue;
            }
            // Within a spell's light: it draws later, lit.
            if lights.iter().any(|(p, r)| mat::box_distance(*p, &m.min, &m.max) < *r) {
                self.lit.push(i);
                continue;
            }
            let mvp = mat::with_bounds(vp, m.min, [m.max[0] - m.min[0], m.max[1] - m.min[1], m.max[2] - m.min[2]]);
            prog.uniforms(ctx, &mvp, 0.0);
            gpu::draw(ctx, self.vtx.add(m.vtx_first as usize * 16), self.idx.add(m.idx_first as usize), m.idx_count);
            stats.draws += 1;
            stats.tris += m.idx_count / 3;
        }
    }

    /// Draws the world from `eye`. The world program and its texture are bound by the caller.
    ///
    /// # Safety
    /// Inside a scene on `ctx`.
    pub unsafe fn draw(&mut self, ctx: *mut g::SceGxmContext, prog: &Program, vp: &Mat4, eye: V3, lod_near: f32, lod_mid: f32, lit: &LitWorld) -> Stats {
        let planes = mat::planes(vp);
        let mut stats = Stats::default();
        self.lit.clear();
        // The lights that are on, as centres and radii.
        let mut lights = [(V3::ZERO, 0.0f32); 4];
        let mut on = 0;
        for k in 0..4 {
            let t = &lit.table[k * 8..k * 8 + 8];
            if t[4] + t[5] + t[6] > 0.01 {
                lights[on] = (V3 { x: t[0], y: t[1], z: t[2] }, 1.0 / requiem_sim::math::sqrt(t[3].max(1e-6)));
                on += 1;
            }
        }
        let lights = &lights[..on];
        self.draw_range(ctx, prog, vp, &planes, self.backdrop, &mut stats, false, &[]);
        for si in 0..self.supers.len() {
            let s = &self.supers[si];
            let (s_far, s_cells, s_min, s_max) = (s.far, s.cells, s.min, s.max);
            let s = Super { min: s_min, max: s_max, far: s_far, cells: s_cells };
            if !mat::visible(&planes, &s.min, &s.max) {
                continue;
            }
            if s.far.count > 0 && mat::box_distance(eye, &s.min, &s.max) > lod_mid {
                let before = stats.draws;
                self.draw_range(ctx, prog, vp, &planes, s.far, &mut stats, true, &[]);
                stats.far += stats.draws - before;
                continue;
            }
            for ci in s.cells.first as usize..(s.cells.first + s.cells.count) as usize {
                let (c_min, c_max, c_near, c_mid) = (self.cells[ci].min, self.cells[ci].max, self.cells[ci].near, self.cells[ci].mid);
                if !mat::visible(&planes, &c_min, &c_max) {
                    continue;
                }
                let before = stats.draws + self.lit.len() as u32;
                if mat::box_distance(eye, &c_min, &c_max) < lod_near {
                    self.draw_range(ctx, prog, vp, &planes, c_near, &mut stats, false, lights);
                    stats.near += stats.draws + self.lit.len() as u32 - before;
                } else {
                    self.draw_range(ctx, prog, vp, &planes, c_mid, &mut stats, false, lights);
                    stats.mid += stats.draws + self.lit.len() as u32 - before;
                }
            }
        }
        // The meshes in a spell's light.
        if !self.lit.is_empty() {
            lit.prog.bind(ctx, false);
            for &i in &self.lit {
                let m = &self.recs[i as usize];
                let size = [m.max[0] - m.min[0], m.max[1] - m.min[1], m.max[2] - m.min[2]];
                let mvp = mat::with_bounds(vp, m.min, size);
                let mut buf = core::ptr::null_mut();
                g::sceGxmReserveVertexDefaultUniformBuffer(ctx, &mut buf);
                if buf.is_null() {
                    continue;
                }
                let bounds = [m.min[0], m.min[1], m.min[2], 0.0, size[0], size[1], size[2], 0.0];
                g::sceGxmSetUniformDataF(buf, lit.prog.vs.param("uMvp"), 0, 16, mvp.as_ptr());
                g::sceGxmSetUniformDataF(buf, lit.bounds, 0, 8, bounds.as_ptr());
                g::sceGxmSetUniformDataF(buf, lit.cast, 0, 32, lit.table.as_ptr());
                gpu::draw(ctx, self.vtx.add(m.vtx_first as usize * 16), self.idx.add(m.idx_first as usize), m.idx_count);
                stats.draws += 1;
                stats.tris += m.idx_count / 3;
                stats.lit += 1;
            }
        }
        stats
    }
}
