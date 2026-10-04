//! The army on the GPU.
//!
//! The pack holds, for each kind of knight and each level of detail, every
//! vertex placed at every stored frame. A frame of the game asks the
//! simulation which knights are in view and which two stored frames each
//! shows, sorts them by (mesh, first frame, second frame), writes one record
//! per knight into this frame's ring memory and issues one instanced draw per
//! run of equal keys. Nothing is skinned and no uniform changes between draws.

use pocket_vita_gxm::mem::{Block, Kind, Ring};
use requiem_pack::{self as pack, CrowdHeader, CrowdInstance, CrowdMesh, CrowdVertex, Pack};
use requiem_sim::crowd::Draw;
use requiem_sim::math::*;
use requiem_sim::Sim;
use vita2d_sys as g;

/// Knights one frame can draw.
pub const CAPACITY: usize = 3072;

struct Mesh {
    vtx_count: usize,
    idx_count: u32,
    color: *const u8,
    idx: *const u16,
    frames: *const u8,
}

#[derive(Clone, Copy, Default)]
pub struct Stats {
    pub shown: u32,
    pub draws: u32,
    pub tris: u32,
    pub by_lod: [u32; 5],
}

pub struct Crowd {
    _block: Block,
    meshes: Vec<Mesh>,
    lods: usize,
    /// Squared distance at which each level of detail hands over to the next.
    reach2: Vec<f32>,
    pub far: f32,
    pub scale: f32,
    draws: Vec<Draw>,
    order: Vec<(u32, u32)>,
    pub bytes: usize,
}

impl Crowd {
    /// # Safety
    /// GXM is initialized.
    pub unsafe fn load(p: &Pack, reach: &[f32]) -> Result<Crowd, String> {
        let data = p.section(pack::CRWD)?;
        let head: CrowdHeader = pack::read(data, 0).ok_or("crowd header")?;
        let count = (head.kinds * head.lods) as usize;
        if reach.len() != head.lods as usize || head.frames > 127 {
            return Err(format!("the pack's crowd has {} levels and {} frames; the profile gives {} distances", head.lods, head.frames, reach.len()));
        }
        let mut block = Block::with_access(Kind::Main, data.len(), false)?;
        let base = block.alloc(data.len(), 16).ok_or("crowd block")?;
        core::ptr::copy_nonoverlapping(data.as_ptr(), base, data.len());
        let mut meshes = Vec::with_capacity(count);
        for i in 0..count {
            let m: CrowdMesh = pack::read(data, core::mem::size_of::<CrowdHeader>() + i * core::mem::size_of::<CrowdMesh>()).ok_or("crowd mesh")?;
            let end = m.frames_at as usize + head.frames as usize * m.vtx_count as usize * core::mem::size_of::<CrowdVertex>();
            if end > data.len() || (m.kind * head.lods + m.lod) as usize != i {
                return Err("the pack's crowd section is malformed".into());
            }
            meshes.push(Mesh { vtx_count: m.vtx_count as usize, idx_count: m.idx_count, color: base.add(m.color_at as usize), idx: base.add(m.idx_at as usize).cast(), frames: base.add(m.frames_at as usize) });
        }
        Ok(Crowd { bytes: block.size(), _block: block, meshes, lods: head.lods as usize, reach2: reach.iter().map(|r| r * r).collect(), far: *reach.last().unwrap_or(&300.0), scale: head.scale, draws: Vec::with_capacity(CAPACITY), order: Vec::with_capacity(CAPACITY) })
    }

    /// Bytes `draw` takes from the ring.
    pub fn frame_bytes() -> usize {
        CAPACITY * core::mem::size_of::<CrowdInstance>() + 64
    }

    /// Draws every knight in view. `bind(far)` binds the program for the near levels of detail (false) or for
    /// the far ones (true) and writes its uniforms; it is called once for each that has knights.
    /// `scale` pulls the hand-over distances in (below 1) to shed load.
    ///
    /// # Safety
    /// Inside a scene on `ctx`; the ring segment is not in use by the GPU.
    pub unsafe fn draw(&mut self, ctx: *mut g::SceGxmContext, sim: &Sim, planes: &[[f32; 4]; 6], eye: V3, ring: &mut Ring, scale: f32, far_from: usize, mut bind: impl FnMut(bool)) -> Stats {
        let mut stats = Stats::default();
        sim.crowd.draw(&sim.field, sim.tick, planes, eye, self.far * scale, &mut self.draws);
        self.order.clear();
        let k2 = scale * scale;
        for (i, d) in self.draws.iter().enumerate().take(CAPACITY) {
            let mut lod = self.lods - 1;
            for (l, r) in self.reach2.iter().enumerate() {
                if d.dist2 < r * k2 {
                    lod = l;
                    break;
                }
            }
            stats.by_lod[lod.min(4)] += 1;
            // Level first, so the draws of one program are together.
            self.order.push(((lod as u32) << 16 | (d.kind as u32).min(2) << 14 | (d.a as u32) << 7 | d.b as u32, i as u32));
        }
        let n = self.order.len();
        if n == 0 {
            return stats;
        }
        self.order.sort_unstable_by_key(|k| k.0);
        let Some(inst) = ring.alloc(n * core::mem::size_of::<CrowdInstance>(), 16) else { return stats };
        let inst = inst.cast::<CrowdInstance>();
        for (k, &(_, i)) in self.order.iter().enumerate() {
            let d = &self.draws[i as usize];
            *inst.add(k) = CrowdInstance {
                pos: [d.pos.x, d.pos.y, d.pos.z],
                turn: [(sin(d.yaw) * 32767.0) as i16, (cos(d.yaw) * 32767.0) as i16],
                blend: (d.blend * 255.0) as u8,
                flash: (d.flash * 255.0) as u8,
                grow: ((d.scale - 1.0) * 255.0) as u8,
                pad: 0,
            };
        }
        let stride = core::mem::size_of::<CrowdVertex>();
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
                bind(far);
                bound = Some(far);
            }
            let m = &self.meshes[((key >> 14) & 3) as usize * self.lods + lod];
            let (a, b) = (((key >> 7) & 127) as usize, (key & 127) as usize);
            g::sceGxmSetVertexStream(ctx, 0, m.frames.add(a * m.vtx_count * stride).cast());
            g::sceGxmSetVertexStream(ctx, 1, m.frames.add(b * m.vtx_count * stride).cast());
            g::sceGxmSetVertexStream(ctx, 2, m.color.cast());
            g::sceGxmSetVertexStream(ctx, 3, inst.add(k).cast());
            let count = (e - k) as u32;
            g::sceGxmDrawInstanced(ctx, g::SceGxmPrimitiveType_SCE_GXM_PRIMITIVE_TRIANGLES, g::SceGxmIndexFormat_SCE_GXM_INDEX_FORMAT_U16, m.idx.cast(), m.idx_count * count, m.idx_count);
            stats.draws += 1;
            stats.tris += m.idx_count / 3 * count;
            k = e;
        }
        stats.shown = n as u32;
        stats
    }
}
