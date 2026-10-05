//! Effects on the GPU.
//!
//! The pack holds each layer of each effect as a template of vertices and
//! eight rows of constants. A frame gathers the live effects by kind, writes
//! one record per live effect into ring memory, and for each layer of each
//! kind with any alive issues one instanced draw: the template as one stream,
//! the records as the other. No particle is stored or stepped.

use pocket_vita_gxm::mem::{Arena, Block, Kind, Ring};
use pocket_vita_gxm::texture::{Format, Texture, Uploader, Wrap};
use requiem_pack::{self as pack, FxEffect, FxHeader, FxInstance, FxLayer, Pack};
use requiem_sim::fx::{FxList, LIFE, SLOTS};
use requiem_sim::math::*;
use requiem_sim::sim::BOLTS;
use requiem_sim::Sim;
use vita2d_sys as g;

use crate::gpu::{self, Program};
use crate::mat::Mat4;

/// Live effects one frame can draw.
const CAPACITY: usize = SLOTS + BOLTS;

struct Layer {
    program: usize,
    over: bool,
    idx_count: u32,
    vtx: *const u8,
    idx: *const u16,
    rows: [f32; 32],
}

/// Uniforms of one effect program.
pub struct FxProgram {
    pub prog: Program,
    mvp: *const g::SceGxmProgramParameter,
    rows: *const g::SceGxmProgramParameter,
    cam: *const g::SceGxmProgramParameter,
    sampler: Option<u32>,
}

impl FxProgram {
    pub fn of(prog: Program) -> FxProgram {
        FxProgram { mvp: prog.vs.param("uMvp"), rows: prog.vs.param("uP"), cam: prog.vs.param("uCam"), sampler: prog.fs.sampler_index("uTex"), prog }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Stats {
    pub live: u32,
    pub draws: u32,
    pub tris: u32,
}

pub struct Fx {
    _block: Block,
    pub atlas: Texture,
    effects: Vec<FxEffect>,
    layers: Vec<Layer>,
    /// Per effect kind this frame: first record and count.
    spans: Vec<(u32, u32)>,
    order: Vec<(u8, FxInstance)>,
}

impl Fx {
    /// # Safety
    /// GXM is initialized; `vram` outlives the texture.
    pub unsafe fn load(p: &Pack, vram: &mut Arena) -> Result<Fx, String> {
        let data = p.section(pack::FXPK)?;
        let head: FxHeader = pack::read(data, 0).ok_or("effects header")?;
        let mut at = core::mem::size_of::<FxHeader>();
        let mut effects = Vec::with_capacity(head.effects as usize);
        for _ in 0..head.effects {
            effects.push(pack::read::<FxEffect>(data, at).ok_or("effect record")?);
            at += core::mem::size_of::<FxEffect>();
        }
        let mut block = Block::with_access(Kind::Main, data.len(), false)?;
        let base = block.alloc(data.len(), 16).ok_or("effects block")?;
        core::ptr::copy_nonoverlapping(data.as_ptr(), base, data.len());
        let mut layers = Vec::with_capacity(head.layers as usize);
        for _ in 0..head.layers {
            let l: FxLayer = pack::read(data, at).ok_or("effect layer")?;
            at += core::mem::size_of::<FxLayer>();
            if l.vtx_at as usize + l.vtx_count as usize * 12 > data.len() || l.idx_at as usize + l.idx_count as usize * 2 > data.len() || l.program > 3 {
                return Err("the pack's effects section is malformed".into());
            }
            layers.push(Layer { program: l.program as usize, over: l.blend == 1, idx_count: l.idx_count, vtx: base.add(l.vtx_at as usize), idx: base.add(l.idx_at as usize).cast(), rows: l.rows });
        }
        // The atlas is one byte of brightness per texel; the sampler reads it from every channel.
        let n = (head.atlas * head.atlas) as usize;
        let src = data.get(head.atlas_at as usize..head.atlas_at as usize + n).ok_or("effects atlas")?;
        let mut rgba = Vec::with_capacity(n * 4);
        for &v in src {
            rgba.extend_from_slice(&[v, v, v, v]);
        }
        let mut up = Uploader::new(n * 4 + 4096)?;
        let mut atlas = up.texture(vram, Format::Rgba8, head.atlas, head.atlas, 1, &rgba)?;
        up.flush();
        up.free();
        atlas.set_wrap(Wrap::Clamp, Wrap::Clamp);
        atlas.set_filter(true, false);
        Ok(Fx { _block: block, atlas, spans: vec![(0, 0); effects.len()], effects, layers, order: Vec::with_capacity(CAPACITY) })
    }

    /// Bytes `draw` takes from the ring.
    pub fn frame_bytes() -> usize {
        CAPACITY * core::mem::size_of::<FxInstance>() + 64
    }

    /// Draws every live effect: the layers that cover first, then the ones that add light.
    ///
    /// # Safety
    /// Inside a scene on `ctx`; the ring segment is not in use by the GPU.
    pub unsafe fn draw(&mut self, ctx: *mut g::SceGxmContext, programs: &[FxProgram; 4], sim: &Sim, vp: &Mat4, eye: V3, right: V3, up: V3, ring: &mut Ring) -> Stats {
        let mut stats = Stats::default();
        // ---- gather by kind
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
        let n = self.order.len().min(CAPACITY);
        stats.live = n as u32;
        if n == 0 {
            return stats;
        }
        self.order.sort_unstable_by_key(|k| k.0);
        let Some(inst) = ring.alloc(n * core::mem::size_of::<FxInstance>(), 16) else { return stats };
        let inst = inst.cast::<FxInstance>();
        for s in self.spans.iter_mut() {
            *s = (0, 0);
        }
        for (k, (kind, record)) in self.order.iter().take(n).enumerate() {
            *inst.add(k) = *record;
            let s = &mut self.spans[*kind as usize];
            if s.1 == 0 {
                s.0 = k as u32;
            }
            s.1 += 1;
        }
        // ---- draw
        gpu::state_overlay(ctx, true);
        let cam = [right.x, right.y, right.z, sim.tick as f32 / 60.0, up.x, up.y, up.z, 0.0, eye.x, eye.y, eye.z, 0.0];
        for over in [true, false] {
            let mut bound = usize::MAX;
            for (kind, e) in self.effects.iter().enumerate() {
                let (first, count) = self.spans[kind];
                if count == 0 {
                    continue;
                }
                for l in &self.layers[e.first as usize..(e.first + e.count) as usize] {
                    if l.over != over {
                        continue;
                    }
                    let p = &programs[l.program];
                    if bound != l.program {
                        // The first fragment program adds; the second covers.
                        p.prog.bind(ctx, over);
                        if let Some(unit) = p.sampler {
                            g::sceGxmSetFragmentTexture(ctx, unit, &self.atlas.gxm);
                        }
                        bound = l.program;
                    }
                    let mut buf = core::ptr::null_mut();
                    g::sceGxmReserveVertexDefaultUniformBuffer(ctx, &mut buf);
                    if buf.is_null() {
                        continue;
                    }
                    g::sceGxmSetUniformDataF(buf, p.mvp, 0, 16, vp.as_ptr());
                    g::sceGxmSetUniformDataF(buf, p.rows, 0, 32, l.rows.as_ptr());
                    if !p.cam.is_null() {
                        g::sceGxmSetUniformDataF(buf, p.cam, 0, 12, cam.as_ptr());
                    }
                    g::sceGxmSetVertexStream(ctx, 0, l.vtx.cast());
                    g::sceGxmSetVertexStream(ctx, 1, inst.add(first as usize).cast());
                    g::sceGxmDrawInstanced(ctx, g::SceGxmPrimitiveType_SCE_GXM_PRIMITIVE_TRIANGLES, g::SceGxmIndexFormat_SCE_GXM_INDEX_FORMAT_U16, l.idx.cast(), l.idx_count * count, l.idx_count);
                    stats.draws += 1;
                    stats.tris += l.idx_count / 3 * count;
                }
            }
        }
        stats
    }
}
