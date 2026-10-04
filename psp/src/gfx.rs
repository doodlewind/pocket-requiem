//! The GE renderer.
//!
//! A frame is one display list, built while the GE draws the previous one:
//!
//! 1. the near pass: the mage, the knights, the ground and the props within
//!    the near distance, with a short frustum and three quarters of the 16-bit
//!    depth buffer;
//! 2. the far pass: everything beyond, with a frustum from 0.8 × that distance
//!    to the horizon and the other quarter. The GE's depth buffer cannot
//!    resolve 0.4 m to 3 km in one range;
//! 3. the sky, the stars and the moon, where nothing was drawn;
//! 4. the shadows and the effects, then the interface.
//!
//! The GE has fixed-function texturing: atlas texel × vertex colour × 2, then
//! linear haze. What the Vita's vertex programs do is done with what the GE
//! has:
//!
//! - A knight is one draw of two morph targets: the pack stores each frame of
//!   the army next to the frame after it, and the GE blends the pair by the
//!   knight's weight. Its light is baked into each frame's colours.
//! - A light a spell casts is a GE point light whose ambient term carries its
//!   colour, so the knights, the ground and the props brighten around it with
//!   no normals.
//! - A struck knight's flash is the haze: for that draw the haze is a constant
//!   share of white.
//! - The effects are evaluated into vertices on the CPU (`requiem_handheld::fx`).
//! - The ground is built from the pack's two grids (`requiem_handheld::ground`);
//!   the squares around the eye are tested against the GE's guard band here.

use core::ffi::c_void;
use core::ptr;

use alloc::vec::Vec;
use requiem_handheld::clip::{ClipVertex, Guard, Verdict, MAX_OUT};
use requiem_handheld::crowd::{CrowdList, Ranks};
use requiem_handheld::figures::{self, ColorVertex};
use requiem_handheld::fx::{self, Fx, FxVertex};
use requiem_handheld::game::Game;
use requiem_handheld::ground::{self, Ground, Grid, Layout, Patch};
use requiem_handheld::hud::{Font, Hud, HudVertex};
use requiem_handheld::mat::{self, Mat4};
use requiem_handheld::scene::{self, Cast};
use requiem_handheld::world::{Pick, World};
use requiem_pack::{self as pack, tex_format, ClipGroup, CrowdHeader, CrowdMesh, HandMesh, ModelHeader, PspBatch, PspCrowdVertex, PspSkinVertex, PspVertex, TexHeader};
use requiem_sim::fx::LIGHTS;
use requiem_sim::math::*;
use requiem_sim::skel::BONES;
use psp::sys::*;
use psp::Align16;

use crate::store;

const LIST_WORDS: usize = 196_608;
static mut LIST: Align16<[u32; LIST_WORDS]> = Align16([0; LIST_WORDS]);

/// One frame buffer: 512 × 272 texels of 16 bits. The colour and depth buffers are all this size.
pub const FB_BYTES: usize = 512 * 272 * 2;
const VRAM_TEXTURES: usize = FB_BYTES * 3;
/// The palettes of an indexed atlas, where the GE reads them: 16-byte aligned main memory.
static mut CLUT: Align16<[[u32; 256]; 4]> = Align16([[0; 256]; 4]);
/// The effects' palette: white, with entry `i` as opaque as `i`.
static mut RAMP: Align16<[u32; 256]> = Align16([0; 256]);
/// Effect vertices and indices a frame may hold.
const FX_VERTS: usize = 3072;
const FX_INDICES: usize = 6144;
/// A ground patch nearer than this has the squares around the eye tested against the guard band.
const GROUND_CLIP: f32 = 21.0;
/// Knights are drawn as far figures out to this distance.
const FAR_REACH: f32 = 380.0;
/// Lights a spell casts that the GE takes: lights 1 and 2.
const CAST: usize = 2;
const VRAM_BYTES: usize = 2 * 1024 * 1024;

pub const MAX_QUADS: usize = 320;
/// Vertices of CPU-clipped triangles per atlas page and pass.
const CLIP_CAP: usize = 1536;
/// The far pass's share of the depth buffer.
const DEPTH_SPLIT: i32 = 16384;
/// The compiler's reach per metre of a large triangle's longest edge (`CLIP_REACH` in requiem-cook).
const CLIP_REACH: f32 = 3.3;

fn vtype_world() -> VertexType {
    VertexType::TEXTURE_16BIT | VertexType::COLOR_5650 | VertexType::VERTEX_16BIT | VertexType::INDEX_16BIT | VertexType::TRANSFORM_3D
}
fn vtype_clip() -> VertexType {
    VertexType::TEXTURE_32BITF | VertexType::COLOR_8888 | VertexType::VERTEX_32BITF | VertexType::TRANSFORM_3D
}
fn vtype_color() -> VertexType {
    VertexType::COLOR_8888 | VertexType::VERTEX_32BITF | VertexType::INDEX_16BIT | VertexType::TRANSFORM_3D
}
fn vtype_skin() -> VertexType {
    VertexType::WEIGHTS4 | VertexType::WEIGHT_8BIT | VertexType::COLOR_8888 | VertexType::NORMAL_8BIT | VertexType::VERTEX_32BITF | VertexType::INDEX_16BIT | VertexType::TRANSFORM_3D
}
/// Two morph targets of `PspCrowdVertex`.
fn vtype_crowd() -> VertexType {
    VertexType::COLOR_5650 | VertexType::VERTEX_16BIT | VertexType::INDEX_16BIT | VertexType::TRANSFORM_3D | VertexType::VERTICES2
}
/// The interface's layout, in the world: the effects.
fn vtype_fx() -> VertexType {
    VertexType::TEXTURE_32BITF | VertexType::COLOR_8888 | VertexType::VERTEX_32BITF | VertexType::INDEX_16BIT | VertexType::TRANSFORM_3D
}
fn vtype_hud() -> VertexType {
    VertexType::TEXTURE_32BITF | VertexType::COLOR_8888 | VertexType::VERTEX_32BITF | VertexType::INDEX_16BIT | VertexType::TRANSFORM_2D
}

fn abgr(c: [f32; 3]) -> u32 {
    let b = |x: f32| (clamp(x, 0.0, 1.0) * 255.0 + 0.5) as u32;
    0xff00_0000 | (b(c[2]) << 16) | (b(c[1]) << 8) | b(c[0])
}

fn fmatrix(m: &Mat4) -> ScePspFMatrix4 {
    let col = |c: usize| ScePspFVector4 { x: m[c], y: m[4 + c], z: m[8 + c], w: m[12 + c] };
    ScePspFMatrix4 { x: col(0), y: col(1), z: col(2), w: col(3) }
}

struct Batch {
    bones: [u8; pack::PSP_BATCH_BONES],
    bone_count: u32,
    vtx: *const u8,
    idx: *const u16,
    idx_count: u32,
}

#[derive(Default)]
struct Model {
    batches: Vec<Batch>,
}

/// One kind of knight at one level of detail: its indices and its frames, each stored with the next of its clip.
struct KnightMesh {
    vtx_count: usize,
    idx_count: u32,
    idx: *const u16,
    frames: *const u8,
}

#[derive(Clone, Copy, Default)]
pub struct Stats {
    pub draws: u32,
    pub tris: u32,
    /// Large triangles tested and cut on the CPU.
    pub tested: u32,
    pub clipped: u32,
    /// Microseconds choosing the props and the ground's patches.
    pub pick_world: u32,
    /// Microseconds of the frame's phases: choosing, the figures and the army near, the ground and props near, the far pass, the sky, shadows and effects, the interface.
    pub phase: [u32; 7],
}

pub struct Gfx {
    tex: TexHeader,
    /// VRAM address of each page's levels.
    pages: Vec<Vec<*const u8>>,
    vtx: Vec<u8>,
    idx: Vec<u16>,
    clip: Vec<u8>,
    _models: Vec<u8>,
    mage: Model,
    demon: Model,
    _crowd: Vec<u8>,
    knights: Vec<KnightMesh>,
    crowd_scale: f32,
    pub list: CrowdList,
    ranks: Ranks,
    _ranks_vb: Vec<u8>,
    fx: Fx,
    fx_tex: *const u8,
    fx_side: i32,
    ground: Ground,
    _ground_vb: Vec<u8>,
    ground_ib: [Vec<u16>; 2],
    patches: Vec<Patch>,
    _font_bytes: Vec<u8>,
    font_tex: *const u8,
    pub font: Font,
    sky_vb: Vec<ColorVertex>,
    sky_ib: Vec<u16>,
    star_vb: Vec<ColorVertex>,
    moon_vb: Vec<ColorVertex>,
    moon_ib: Vec<u16>,
    quad_ib: Vec<u16>,
    fan_ib: Vec<u16>,
    far: Vec<Pick>,
    near: Vec<Pick>,
    pub stats: Stats,
    /// Meshes chosen by level this frame.
    pub picked: requiem_handheld::world::Stats,
    /// Leaves every triangle to the GE (`option=4`), to see what its guard band drops.
    no_clip: bool,
    scratch: Scratch,
    pub resident_bytes: usize,
}

/// This frame's vertices and indices that are computed on the CPU: one block of ordinary memory, filled
/// from the start each frame. The display list's own memory is addressed past the data cache, where a
/// write costs a bus cycle; here a batch is written through the cache and flushed once, before its draw.
struct Scratch {
    mem: Vec<u8>,
    at: usize,
}

impl Scratch {
    const BYTES: usize = 512 * 1024;
    fn new() -> Scratch {
        Scratch { mem: alloc::vec![0u8; Scratch::BYTES + 64], at: 0 }
    }
    /// `bytes` on a 16-byte boundary. A frame that asks for more than the block starts over: its early
    /// batches are drawn by then.
    unsafe fn take(&mut self, bytes: i32) -> *mut c_void {
        let base = self.mem.as_mut_ptr();
        let first = (base as usize + 63) & !63;
        let mut at = (first + self.at + 15) & !15;
        if at + bytes as usize > base as usize + self.mem.len() {
            at = first;
        }
        self.at = at + bytes as usize - first;
        at as *mut c_void
    }
}

unsafe fn flush<T>(p: *const T, bytes: usize) {
    sceKernelDcacheWritebackRange(p as *const c_void, bytes as u32);
}

unsafe fn load_model(models: &[u8], o: &mut usize) -> Result<(u32, Model), &'static str> {
    let h: ModelHeader = pack::read(models, *o).ok_or("model header")?;
    *o += core::mem::size_of::<ModelHeader>();
    let mut model = Model::default();
    for _ in 0..h.pad {
        let b: PspBatch = pack::read(models, *o).ok_or("model batch")?;
        *o += core::mem::size_of::<PspBatch>();
        let vb = b.vtx_count as usize * core::mem::size_of::<PspSkinVertex>();
        let ib = b.idx_count as usize * 2;
        if *o + vb + ib > models.len() {
            return Err("model section is truncated");
        }
        model.batches.push(Batch { bones: b.bones, bone_count: b.bone_count, vtx: models.as_ptr().add(*o), idx: models.as_ptr().add(*o + vb).cast(), idx_count: b.idx_count });
        *o = (*o + vb + ib + 3) & !3;
    }
    Ok((h.id, model))
}

impl Gfx {
    /// Reads the pack's drawing data and sets the GE up.
    pub unsafe fn load(file: &store::PackFile, scene: &requiem_handheld::scene::Scene, sim: &requiem_sim::Sim, progress: &mut dyn FnMut(&str)) -> Result<Gfx, &'static str> {
        let field = &sim.field;
        // The atlas goes to VRAM after the two frame buffers and the depth buffer.
        progress("atlas");
        let tex_bytes: Vec<u8> = file.records(pack::TEX0)?;
        let tex: TexHeader = pack::read(&tex_bytes, 0).ok_or("atlas header")?;
        if !matches!(tex.format, tex_format::PSP_DXT1 | tex_format::PSP_5650 | tex_format::PSP_T8) || scene.h.pages > 4 {
            return Err("the atlas is not a PSP texture");
        }
        let vram = sceGeEdramGetAddr();
        let mut at = (VRAM_TEXTURES + 15) & !15;
        let mut src = core::mem::size_of::<TexHeader>();
        let mut pages = Vec::new();
        for page in 0..scene.h.pages as usize {
            let mut levels = Vec::new();
            if tex.format == tex_format::PSP_T8 {
                src = (src + 15) & !15;
                if src + tex_format::PALETTE_BYTES > tex_bytes.len() {
                    return Err("the atlas is truncated");
                }
                ptr::copy_nonoverlapping(tex_bytes.as_ptr().add(src), ptr::addr_of_mut!(CLUT.0[page]) as *mut u8, tex_format::PALETTE_BYTES);
                src += tex_format::PALETTE_BYTES;
            }
            for l in 0..tex.mips {
                src = (src + 15) & !15;
                let n = tex_format::level_bytes(tex.format, tex.width >> l, tex.height >> l);
                if at + n > VRAM_BYTES || src + n > tex_bytes.len() {
                    return Err("the atlas does not fit in video memory");
                }
                ptr::copy_nonoverlapping(tex_bytes.as_ptr().add(src), vram.add(at), n);
                levels.push(vram.add(at) as *const u8);
                at = (at + n + 15) & !15;
                src += n;
            }
            pages.push(levels);
        }
        drop(tex_bytes);

        // The effects' atlas, after it: indices of a ramp from clear to opaque white.
        progress("effects");
        let fx_bytes: Vec<u8> = file.records(pack::FXTX)?;
        let fx_head: TexHeader = pack::read(&fx_bytes, 0).ok_or("effects atlas header")?;
        let n = tex_format::level_bytes(fx_head.format, fx_head.width, fx_head.height);
        if fx_head.format != tex_format::PSP_T8 || at + n > VRAM_BYTES || 16 + n > fx_bytes.len() {
            return Err("the effects' atlas is not a PSP texture, or video memory is full");
        }
        ptr::copy_nonoverlapping(fx_bytes.as_ptr().add(16), vram.add(at), n);
        let fx_tex = vram.add(at) as *const u8;
        drop(fx_bytes);
        for (i, c) in (*ptr::addr_of_mut!(RAMP.0)).iter_mut().enumerate() {
            *c = (i as u32) << 24 | 0x00ff_ffff;
        }
        let fxpk: Vec<u8> = file.records(pack::FXPK)?;
        let mut fx = Fx::parse(&fxpk)?;
        fx.far = 230.0;
        drop(fxpk);

        progress("props");
        let vtx: Vec<u8> = file.records(pack::VTX0)?;
        let idx: Vec<u16> = file.records(pack::IDX0)?;
        let clip: Vec<u8> = file.records(pack::CLIP)?;

        progress("ground");
        let grid_bytes: Vec<u8> = file.records(pack::GRND)?;
        let grid = Grid::parse(&grid_bytes)?;
        drop(grid_bytes);
        let mut ground_vb = alloc::vec![0u8; Ground::bytes(Layout::Psp)];
        let ground = Ground::new(grid, field, Layout::Psp, scene.h.cell, scene.h.super_cell, scene.h.u_range, ground_vb.as_mut_ptr())?;
        let mut ground_ib = [alloc::vec![0u16; ground::index_count(ground::NEAR_N)], alloc::vec![0u16; ground::index_count(ground::SMALL_N)]];
        ground::indices(ground::NEAR_N, &mut ground_ib[0]);
        ground::indices(ground::SMALL_N, &mut ground_ib[1]);

        progress("models");
        let models: Vec<u8> = file.records(pack::MODL)?;
        let count: u32 = pack::read(&models, 0).ok_or("model count")?;
        let (mut mage, mut demon) = (Model::default(), Model::default());
        let mut o = 4;
        for _ in 0..count {
            match load_model(&models, &mut o)? {
                (0, m) => mage = m,
                (4, m) => demon = m,
                _ => {}
            }
        }
        if mage.batches.is_empty() {
            return Err("the pack lacks the mage's model");
        }

        progress("army");
        let crowd: Vec<u8> = file.records(pack::CRWP)?;
        let head: CrowdHeader = pack::read(&crowd, 0).ok_or("crowd header")?;
        let mut knights = Vec::new();
        let mut tris = Vec::new();
        for i in 0..(head.kinds * head.lods) as usize {
            let m: CrowdMesh = pack::read(&crowd, core::mem::size_of::<CrowdHeader>() + i * core::mem::size_of::<CrowdMesh>()).ok_or("crowd mesh")?;
            let end = m.frames_at as usize + head.frames as usize * m.vtx_count as usize * 2 * core::mem::size_of::<PspCrowdVertex>();
            if end > crowd.len() || (m.kind * head.lods + m.lod) as usize != i {
                return Err("the pack's crowd section is malformed");
            }
            knights.push(KnightMesh { vtx_count: m.vtx_count as usize, idx_count: m.idx_count, idx: crowd.as_ptr().add(m.idx_at as usize).cast(), frames: crowd.as_ptr().add(m.frames_at as usize) });
            tris.push(m.idx_count / 3);
        }
        if head.lods != scene.h.crowd_lods || head.kinds != 3 {
            return Err("the pack's crowd does not match its scene");
        }
        let list = CrowdList::new(&scene.h, tris, true);
        let mut ranks_vb = alloc::vec![0u8; Ranks::bytes(sim)];
        let ranks = Ranks::new(sim, ranks_vb.as_mut_ptr());

        progress("interface");
        let font_bytes: Vec<u8> = file.records(pack::FONT)?;
        let (font, tex_at) = Font::parse(&font_bytes)?;
        if font.format != tex_format::PSP_4444 {
            return Err("the font atlas is not a PSP texture");
        }
        let font_tex = font_bytes.as_ptr().add(tex_at);

        let mut sky_vb = alloc::vec![ColorVertex::default(); figures::SKY_VERTS];
        let mut sky_ib = alloc::vec![0u16; figures::SKY_INDICES];
        figures::sky(scene, 1000.0, &mut sky_vb, &mut sky_ib);
        let mut star_vb = alloc::vec![ColorVertex::default(); figures::STAR_VERTS];
        // The view is 58 degrees over 272 lines.
        figures::stars(960.0, 58.0 * (PI / 180.0) / 272.0, &mut star_vb);
        let mut moon_vb = alloc::vec![ColorVertex::default(); figures::MOON_VERTS];
        let mut moon_ib = alloc::vec![0u16; figures::MOON_INDICES];
        figures::moon(scene, 940.0, &mut moon_vb, &mut moon_ib);
        let quads = MAX_QUADS.max(figures::STARS).max(requiem_handheld::crowd::FAR_FIGURES * 2);
        let mut quad_ib = alloc::vec![0u16; quads * 6];
        figures::quad_indices(&mut quad_ib, quads);
        let mut fan_ib = alloc::vec![0u16; figures::DISC_INDICES];
        figures::fan_indices(&mut fan_ib, figures::DISCS, figures::FAN);

        let resident_bytes = vtx.len() + idx.len() * 2 + clip.len() + models.len() + crowd.len() + ranks_vb.len() + ground_vb.len() + font_bytes.len();
        sceKernelDcacheWritebackAll();

        sceGuInit();
        sceGuStart(GuContextType::Direct, ptr::addr_of_mut!(LIST.0) as *mut c_void);
        // 16-bit colour with ordered dither: half the memory traffic of 32-bit per pixel written.
        sceGuDrawBuffer(DisplayPixelFormat::Psm5650, ptr::null_mut(), 512);
        sceGuDispBuffer(480, 272, FB_BYTES as *mut c_void, 512);
        sceGuDepthBuffer((FB_BYTES * 2) as *mut c_void, 512);
        sceGuOffset(2048 - 240, 2048 - 136);
        sceGuViewport(2048, 2048, 480, 272);
        sceGuDepthRange(65535, 0);
        sceGuDepthFunc(DepthFunc::GreaterOrEqual);
        sceGuScissor(0, 0, 480, 272);
        sceGuEnable(GuState::ScissorTest);
        sceGuEnable(GuState::ClipPlanes);
        sceGuFrontFace(FrontFaceDirection::CounterClockwise);
        sceGuShadeModel(ShadingModel::Smooth);
        let row = |x, y, z, w| ScePspIVector4 { x, y, z, w };
        sceGuSetDither(&ScePspIMatrix4 { x: row(-4, 0, -3, 1), y: row(2, -2, 3, -1), z: row(-3, 1, -4, 0), w: row(3, -1, 2, -2) });
        sceGuEnable(GuState::Dither);
        sceGuTexWrap(GuTexWrapMode::Repeat, GuTexWrapMode::Clamp);
        sceGuBlendFunc(BlendOp::Add, BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha, 0, 0);
        sceGuLightMode(LightMode::SingleColor);
        sceGuColorMaterial(LightComponent::AMBIENT | LightComponent::DIFFUSE);
        sceGuFinish();
        sceGuSync(GuSyncMode::Finish, GuSyncBehavior::Wait);
        sceDisplayWaitVblankStart();
        sceGuDisplay(true);

        Ok(Gfx {
            tex,
            pages,
            vtx,
            idx,
            clip,
            _models: models,
            mage,
            demon,
            _crowd: crowd,
            knights,
            crowd_scale: head.scale,
            list,
            ranks,
            _ranks_vb: ranks_vb,
            fx,
            fx_tex,
            fx_side: fx_head.width as i32,
            ground,
            _ground_vb: ground_vb,
            ground_ib,
            patches: Vec::with_capacity(160),
            _font_bytes: font_bytes,
            font_tex,
            font,
            sky_vb,
            sky_ib,
            star_vb,
            moon_vb,
            moon_ib,
            quad_ib,
            fan_ib,
            far: Vec::with_capacity(512),
            near: Vec::with_capacity(128),
            stats: Stats::default(),
            picked: Default::default(),
            no_clip: false,
            scratch: Scratch::new(),
            resident_bytes,
        })
    }

    unsafe fn bind_page(&self, page: usize) {
        let (format, swizzled) = match self.tex.format {
            tex_format::PSP_DXT1 => (TexturePixelFormat::PsmDxt1, 0),
            tex_format::PSP_T8 => {
                sceGuClutMode(ClutPixelFormat::Psm8888, 0, 0xff, 0);
                sceGuClutLoad(32, ptr::addr_of!(CLUT.0[page]) as *const c_void);
                (TexturePixelFormat::PsmT8, 1)
            }
            _ => (TexturePixelFormat::Psm5650, 1),
        };
        sceGuTexMode(format, self.tex.mips as i32 - 1, 0, swizzled);
        const LEVELS: [MipmapLevel; 8] = [MipmapLevel::None, MipmapLevel::Level1, MipmapLevel::Level2, MipmapLevel::Level3, MipmapLevel::Level4, MipmapLevel::Level5, MipmapLevel::Level6, MipmapLevel::Level7];
        for (l, p) in self.pages[page].iter().enumerate() {
            let (w, h) = ((self.tex.width >> l) as i32, (self.tex.height >> l) as i32);
            sceGuTexImage(LEVELS[l], w, h, w, *p as *const c_void);
        }
    }

    /// One static mesh: the GE draws what it can take; large triangles near the eye are tested and cut here.
    #[allow(clippy::too_many_arguments)]
    unsafe fn mesh(&mut self, rec: &HandMesh, vtx: *const u8, idx: *const u16, eye: V3, look: V3, guard: &Guard, u_range: f32, out: *mut ClipVertex, out_n: &mut usize) {
        let (s, t) = mat::dequant(&rec.min, &rec.max, 32768.0);
        let world = ScePspFMatrix4 {
            x: ScePspFVector4 { x: s[0], y: 0.0, z: 0.0, w: 0.0 },
            y: ScePspFVector4 { x: 0.0, y: s[1], z: 0.0, w: 0.0 },
            z: ScePspFVector4 { x: 0.0, y: 0.0, z: s[2], w: 0.0 },
            w: ScePspFVector4 { x: t[0], y: t[1], z: t[2], w: 1.0 },
        };
        sceGuSetMatrix(MatrixMode::Model, &world);
        let nbig = ((rec.idx_count - rec.big_first) / 3) as usize;
        let near_mesh = nbig > 0 && !self.no_clip && mat::box_distance(eye, &rec.min, &rec.max) < rec.clip_radius;
        if !near_mesh {
            sceGuDrawArray(GuPrimitive::Triangles, vtype_world(), rec.idx_count as i32, idx.cast(), vtx.cast());
            self.stats.draws += 1;
            self.stats.tris += rec.idx_count / 3;
            return;
        }
        // The mesh's large triangles, group by group. Inside a group the largest come first, so the
        // ones that could reach the guard band from this distance are a prefix; the rest of the group
        // draws as it is, in one call with the neighbouring groups' rests where they touch.
        let table = self.clip.as_ptr().add(rec.clip_first as usize);
        let group_count = (table as *const u32).read_unaligned() as usize;
        let groups = table.add(4) as *const ClipGroup;
        let codes = table.add(4 + group_count * core::mem::size_of::<ClipGroup>());
        let mut tested = 0usize;
        // The static run being gathered: the small triangles, then untested large ones.
        let (mut run_first, mut run_len) = (0usize, rec.big_first as usize);
        let mut prefix = [0u16; 64];
        let mut at = 0usize;
        for g in 0..group_count {
            let group = groups.add(g).read_unaligned();
            let d = mat::box_distance(eye, &group.min, &group.max);
            let mut n = 0usize;
            while n < group.tris as usize {
                let c = *codes.add(at + n);
                if c != 255 && c as f32 * 2.0 <= d {
                    break;
                }
                n += 1;
            }
            if g < prefix.len() {
                prefix[g] = n as u16;
            }
            tested += n;
            // Indices of this group's untested rest.
            let rest_first = rec.big_first as usize + (at + n) * 3;
            let rest_len = (group.tris as usize - n) * 3;
            if n == 0 && run_first + run_len == rest_first {
                run_len += rest_len;
            } else {
                if run_len > 0 {
                    sceGuDrawArray(GuPrimitive::Triangles, vtype_world(), run_len as i32, idx.add(run_first).cast(), vtx.cast());
                    self.stats.draws += 1;
                    self.stats.tris += (run_len / 3) as u32;
                }
                run_first = rest_first;
                run_len = rest_len;
            }
            at += group.tris as usize;
        }
        if run_len > 0 {
            sceGuDrawArray(GuPrimitive::Triangles, vtype_world(), run_len as i32, idx.add(run_first).cast(), vtx.cast());
            self.stats.draws += 1;
            self.stats.tris += (run_len / 3) as u32;
        }
        if tested == 0 {
            return;
        }

        let verts = vtx as *const PspVertex;
        let k = [s[0] / 32768.0, s[1] / 32768.0, s[2] / 32768.0];
        let pos = |i: u16| {
            let v = &*verts.add(i as usize);
            v3(v.pos[0] as f32 * k[0] + t[0], v.pos[1] as f32 * k[1] + t[1], v.pos[2] as f32 * k[2] + t[2])
        };
        let full = |i: u16, p: V3| {
            let v = &*verts.add(i as usize);
            let c = v.color;
            let (r, g, b) = ((c & 31) as u8, ((c >> 5) & 63) as u8, (c >> 11) as u8);
            ClipVertex { uv: [v.uv[0] as f32 * (u_range / 32768.0), v.uv[1] as f32 / 32768.0], color: [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2), 255], pos: [p.x, p.y, p.z] }
        };
        let near = guard.near();
        let keep = self.scratch.take((tested * 6) as i32) as *mut u16;
        let mut kept = 0usize;
        let first = idx.add(rec.big_first as usize);
        let mut at = 0usize;
        for g in 0..group_count {
            let group_tris = groups.add(g).read_unaligned().tris as usize;
            let n = if g < prefix.len() { prefix[g] as usize } else { 0 };
            for j in at..at + n {
                let (a, b, c) = (*first.add(j * 3), *first.add(j * 3 + 1), *first.add(j * 3 + 2));
                let code = *codes.add(j);
                let pa = pos(a);
                let reach = code as f32 * 2.0;
                let to = pa - eye;
                let d2 = to.len2();
                let mut take = code != 255 && d2 > reach * reach;
                // Before three transforms: `edge` bounds the triangle's size, so depth along the view decides
                // most. Wholly behind the near plane, it is not drawn; wholly inside a cone of 82 degrees
                // about the view direction (the guard band is wider than that at every field of view the
                // camera uses), the GE takes it.
                let mut slow = !take;
                if slow && code != 255 {
                    let edge = reach * (1.0 / CLIP_REACH);
                    let z = to.dot(look);
                    if z + edge < near {
                        continue;
                    }
                    if z - edge > near && z - edge > 0.14 * (sqrt(d2) + edge) {
                        take = true;
                        slow = false;
                    }
                }
                if slow {
                    let (pb, pc) = (pos(b), pos(c));
                    let cc = [guard.to_clip(pa), guard.to_clip(pb), guard.to_clip(pc)];
                    match guard.classify(&cc) {
                        Verdict::Safe => take = true,
                        Verdict::Culled => {}
                        Verdict::Clip => {
                            if *out_n + MAX_OUT <= CLIP_CAP {
                                let tri = [full(a, pa), full(b, pb), full(c, pc)];
                                let n = guard.clip(&tri, &cc, core::slice::from_raw_parts_mut(out.add(*out_n), MAX_OUT));
                                *out_n += n;
                                self.stats.clipped += 1;
                            } else {
                                take = true;
                            }
                        }
                    }
                }
                if take {
                    *keep.add(kept) = a;
                    *keep.add(kept + 1) = b;
                    *keep.add(kept + 2) = c;
                    kept += 3;
                }
            }
            at += group_tris;
        }
        self.stats.tested += tested as u32;
        if kept > 0 {
            flush(keep, kept * 2);
            sceGuDrawArray(GuPrimitive::Triangles, vtype_world(), kept as i32, keep.cast(), vtx.cast());
            self.stats.draws += 1;
            self.stats.tris += (kept / 3) as u32;
        }
    }

    /// The props of one list, page by page, then the pieces the CPU cut from them and from the ground.
    #[allow(clippy::too_many_arguments)]
    unsafe fn meshes(&mut self, world: &World, game: &Game, near_list: bool, eye: V3, look: V3, guard: &Guard, u_range: f32) {
        for page in 0..self.pages.len() {
            let out = self.scratch.take((CLIP_CAP * core::mem::size_of::<ClipVertex>()) as i32) as *mut ClipVertex;
            let mut out_n = 0usize;
            self.bind_page(page);
            // The ground first: it is under everything, and what stands on it then hides it before it is textured twice.
            if page == self.ground.grid.head.page as usize {
                for i in 0..self.patches.len() {
                    let p = self.patches[i];
                    if (p.level == ground::level::NEAR) == near_list {
                        self.patch(game, &p, eye, guard, out, &mut out_n);
                    }
                }
            }
            let count = if near_list { self.near.len() } else { self.far.len() };
            for i in 0..count {
                let pick = if near_list { self.near[i] } else { self.far[i] };
                let rec = world.recs[pick.mesh as usize];
                if rec.page as usize != page {
                    continue;
                }
                let (vtx, idx) = (self.vtx.as_ptr().add(rec.vtx_first as usize * core::mem::size_of::<PspVertex>()), self.idx.as_ptr().add(rec.idx_first as usize));
                self.mesh(&rec, vtx, idx, eye, look, guard, u_range, out, &mut out_n);
            }
            if out_n > 0 {
                flush(out, out_n * core::mem::size_of::<ClipVertex>());
                sceGuSetMatrix(MatrixMode::Model, &fmatrix(&mat::IDENTITY));
                sceGuTexScale(1.0, 1.0);
                sceGuDrawArray(GuPrimitive::Triangles, vtype_clip(), out_n as i32, ptr::null(), out.cast());
                sceGuTexScale(u_range, 1.0);
                self.stats.draws += 1;
                self.stats.tris += (out_n / 3) as u32;
            }
        }
    }

    /// One patch of the ground. A near patch around the eye has its squares within `GROUND_CLIP` tested
    /// against the guard band: the GE drops a triangle with a vertex outside it.
    unsafe fn patch(&mut self, game: &Game, p: &Patch, eye: V3, guard: &Guard, out: *mut ClipVertex, out_n: &mut usize) {
        let (s, t) = mat::dequant(&p.min, &p.max, 32768.0);
        let world = ScePspFMatrix4 {
            x: ScePspFVector4 { x: s[0], y: 0.0, z: 0.0, w: 0.0 },
            y: ScePspFVector4 { x: 0.0, y: s[1], z: 0.0, w: 0.0 },
            z: ScePspFVector4 { x: 0.0, y: 0.0, z: s[2], w: 0.0 },
            w: ScePspFVector4 { x: t[0], y: t[1], z: t[2], w: 1.0 },
        };
        sceGuSetMatrix(MatrixMode::Model, &world);
        let vtx = self.ground.vertex(p.offset);
        if p.built != 0 {
            let n = if p.level == ground::level::NEAR { ground::NEAR_N } else { ground::SMALL_N };
            flush(vtx, ground::verts(n) * core::mem::size_of::<PspVertex>());
        }
        let near = p.level == ground::level::NEAR;
        let which = if near { 0 } else { 1 };
        let (ib, ib_len) = (self.ground_ib[which].as_ptr(), self.ground_ib[which].len());
        if !near || p.dist > GROUND_CLIP || self.no_clip {
            sceGuDrawArray(GuPrimitive::Triangles, vtype_world(), ib_len as i32, ib.cast(), vtx.cast());
            self.stats.draws += 1;
            self.stats.tris += (ib_len / 3) as u32;
            return;
        }
        let n = ground::NEAR_N;
        let field = &game.sim.field;
        let keep = self.scratch.take((ib_len * 2) as i32) as *mut u16;
        let mut kept = 0usize;
        let size = (p.max[0] - p.min[0]) / n as f32;
        let reach2 = GROUND_CLIP * GROUND_CLIP;
        for b in 0..n {
            for a in 0..n {
                let first = (b * n + a) * 6;
                let (cx, cz) = (p.min[0] + (a as f32 + 0.5) * size, p.min[2] + (b as f32 + 0.5) * size);
                let (dx, dz) = (cx - eye.x, cz - eye.z);
                if dx * dx + dz * dz > reach2 {
                    ptr::copy_nonoverlapping(ib.add(first), keep.add(kept), 6);
                    kept += 6;
                    continue;
                }
                // The square's four corners, then its two triangles as the index list has them.
                let c = [(a, b), (a + 1, b + 1), (a + 1, b), (a, b + 1)].map(|(i, j)| self.ground.corner(field, p.level, p.cx as usize, p.cz as usize, i, j));
                let cc = [guard.to_clip(c[0].0), guard.to_clip(c[1].0), guard.to_clip(c[2].0), guard.to_clip(c[3].0)];
                for (k, tri) in [[0usize, 1, 2], [0, 3, 1]].into_iter().enumerate() {
                    let tc = [cc[tri[0]], cc[tri[1]], cc[tri[2]]];
                    self.stats.tested += 1;
                    match guard.classify(&tc) {
                        Verdict::Safe => {
                            ptr::copy_nonoverlapping(ib.add(first + k * 3), keep.add(kept), 3);
                            kept += 3;
                        }
                        Verdict::Culled => {}
                        Verdict::Clip => {
                            if *out_n + MAX_OUT <= CLIP_CAP {
                                let v = tri.map(|i| ClipVertex { uv: c[i].2, color: c[i].1, pos: [c[i].0.x, c[i].0.y, c[i].0.z] });
                                *out_n += guard.clip(&v, &tc, core::slice::from_raw_parts_mut(out.add(*out_n), MAX_OUT));
                                self.stats.clipped += 1;
                            }
                        }
                    }
                }
            }
        }
        // The skirt, as it is.
        let skirt = n * n * 6;
        ptr::copy_nonoverlapping(ib.add(skirt), keep.add(kept), ib_len - skirt);
        kept += ib_len - skirt;
        flush(keep, kept * 2);
        sceGuDrawArray(GuPrimitive::Triangles, vtype_world(), kept as i32, keep.cast(), vtx.cast());
        self.stats.draws += 1;
        self.stats.tris += (kept / 3) as u32;
    }

    unsafe fn model(&mut self, demon: bool, skin: &[M34; BONES]) {
        let model = if demon { &self.demon } else { &self.mage };
        let (mut draws, mut tris) = (0, 0);
        for b in &model.batches {
            for (slot, &bone) in b.bones[..b.bone_count as usize].iter().enumerate() {
                let m = &skin[bone as usize];
                let col = |v: V3, w: f32| ScePspFVector4 { x: v.x, y: v.y, z: v.z, w };
                let fm = ScePspFMatrix4 { x: col(m.r.x, 0.0), y: col(m.r.y, 0.0), z: col(m.r.z, 0.0), w: col(m.t, 1.0) };
                sceGuBoneMatrix(slot as u32, &fm);
            }
            sceGuDrawArray(GuPrimitive::Triangles, vtype_skin(), b.idx_count as i32, b.idx.cast(), b.vtx.cast());
            draws += 1;
            tris += b.idx_count / 3;
        }
        self.stats.draws += draws;
        self.stats.tris += tris;
    }

    /// The knights of the list as far as `split` when `near`, beyond it otherwise.
    /// A knight is one draw: its place and heading as the model matrix, its two frames as two morph targets.
    unsafe fn knights(&mut self, near: bool, split: f32, fog: (f32, f32, u32)) {
        let stride = 2 * core::mem::size_of::<PspCrowdVertex>();
        let (mut draws, mut tris) = (0, 0);
        for k in &self.list.out {
            if (k.dist <= split) != near {
                continue;
            }
            let m = &self.knights[k.mesh as usize];
            let s = self.crowd_scale * k.scale;
            let (sn, cs) = (k.turn[0] * s, k.turn[1] * s);
            let world = ScePspFMatrix4 {
                x: ScePspFVector4 { x: cs, y: 0.0, z: -sn, w: 0.0 },
                y: ScePspFVector4 { x: 0.0, y: s, z: 0.0, w: 0.0 },
                z: ScePspFVector4 { x: sn, y: 0.0, z: cs, w: 0.0 },
                w: ScePspFVector4 { x: k.pos[0], y: k.pos[1], z: k.pos[2], w: 1.0 },
            };
            sceGuSetMatrix(MatrixMode::Model, &world);
            sceGuMorphWeight(0, 1.0 - k.blend);
            sceGuMorphWeight(1, k.blend);
            let flash = k.flash > 0.03;
            if flash {
                // A constant share of white: the haze's range moved so far out that depth no longer changes it.
                let far = k.dist + (1.0 - 0.6 * min(k.flash, 1.0)) * 1e5;
                sceGuFog(far - 1e5, far, 0xffff_ebdc);
            }
            sceGuDrawArray(GuPrimitive::Triangles, vtype_crowd(), m.idx_count as i32, m.idx.cast(), m.frames.add(k.a as usize * m.vtx_count * stride).cast());
            if flash {
                sceGuFog(fog.0, fog.1, fog.2);
            }
            draws += 1;
            tris += m.idx_count / 3;
        }
        self.stats.draws += draws;
        self.stats.tris += tris;
    }

    /// Lights 1 and 2 as the spells' lights. `figure`: with a diffuse term, for a model with normals;
    /// otherwise the ambient term alone carries the colour, for geometry without.
    unsafe fn cast_lights(&self, cast: &[Cast; LIGHTS], n: usize, figure: bool, scene: &requiem_handheld::scene::Scene) {
        const LIGHT: [GuState; CAST] = [GuState::Light1, GuState::Light2];
        for k in 0..CAST {
            if k >= n {
                sceGuDisable(LIGHT[k]);
                continue;
            }
            let c = &cast[k];
            let i = k as i32 + 1;
            sceGuEnable(LIGHT[k]);
            sceGuLight(i, LightType::Pointlight, LightComponent::AMBIENT | LightComponent::DIFFUSE, &ScePspFVector3 { x: c.pos[0], y: c.pos[1], z: c.pos[2] });
            // 1 / (1 + 5 d² / r²) stands in for the programs' (1 - d² / r²)².
            sceGuLightAtt(i, 1.0, 0.0, 5.0 * c.inv_r2);
            let enc = |x: f32| scene.encode(x) as f32 * (1.0 / 255.0);
            // Baked colours take half the light: they are near white under the moon already.
            let (amb, dif) = if figure { (0.36, 0.84) } else { (0.45, 0.0) };
            sceGuLightColor(i, LightComponent::AMBIENT, abgr([enc(c.color[0] * amb), enc(c.color[1] * amb), enc(c.color[2] * amb)]));
            sceGuLightColor(i, LightComponent::DIFFUSE, abgr([enc(c.color[0] * dif), enc(c.color[1] * dif), enc(c.color[2] * dif)]));
        }
    }

    /// Builds and submits the frame's display list. The GE draws it while the caller prepares the next frame.
    pub unsafe fn frame(&mut self, game: &mut Game, world: &World, perf: &requiem_handheld::game::Perf) {
        self.stats = Stats::default();
        self.scratch.at = 0;
        self.no_clip = game.set.option & 4 != 0;
        let frame_start = sceKernelGetSystemTimeLow();
        let mut mark = frame_start;
        let mut phase = [0u32; 7];
        let mut lap = |i: usize| {
            let now = sceKernelGetSystemTimeLow();
            phase[i] = now.wrapping_sub(mark);
            mark = now;
        };
        let cam = game.camera();
        let scene_h = game.scene.h;
        let (lod_near, lod_mid, lod_far) = game.lod();
        let aspect = game.aspect();
        let view = mat::view(cam.eye, cam.look, cam.roll);
        let vp = mat::mul(&mat::perspective(cam.fov, aspect, scene_h.clip_near, scene_h.clip_far), &view);
        let planes = mat::planes(&vp);

        // ------------------------------------------------------------ what this frame draws
        self.far.clear();
        self.near.clear();
        self.patches.clear();
        game.info = Default::default();
        if game.set.world {
            self.picked = world.pick(&planes, cam.eye, lod_near, lod_mid, lod_far, &|_| true, &mut self.far, &mut self.near);
            self.ground.pick(&game.sim.field, &planes, cam.eye, lod_near, lod_mid, lod_far, game.frame, &mut self.patches);
            game.info.patches = self.patches.len() as u32;
            game.info.built = self.ground.built;
            game.info.props = self.picked.near + self.picked.mid + self.picked.far;
        }
        let pick_world = sceKernelGetSystemTimeLow().wrapping_sub(frame_start);
        if game.set.crowd {
            game.info.crowd = self.list.build(&game.sim, &planes, cam.eye, game.lod_scale);
        } else {
            self.list.out.clear();
        }
        let mut cast = [Cast::default(); LIGHTS];
        let lit = scene::cast(&game.sim, cam.eye, &mut cast).min(CAST);
        lap(0);

        sceGuStart(GuContextType::Direct, ptr::addr_of_mut!(LIST.0) as *mut c_void);
        sceGuDepthMask(0);
        let fog_color = abgr(game.scene.fog_srgb());
        sceGuClearColor(fog_color);
        sceGuClearDepth(0);
        sceGuClear(ClearBuffer::COLOR_BUFFER_BIT | ClearBuffer::DEPTH_BUFFER_BIT);
        sceGuSetMatrix(MatrixMode::View, &fmatrix(&view));
        let fog = (scene_h.fog_near, scene_h.fog_far, fog_color);

        let far_near = max(lod_near * 0.8, 8.0);
        let far_proj = fmatrix(&mat::perspective_gl(cam.fov, aspect, far_near, scene_h.clip_far));
        let near_proj = fmatrix(&mat::perspective_gl(cam.fov, aspect, scene_h.clip_near, lod_near + 180.0));
        sceGuDisable(GuState::Blend);
        // Front to back, so the depth test rejects what is hidden before it is textured.
        let by_distance = |a: &Pick, b: &Pick| a.dist.partial_cmp(&b.dist).unwrap_or(core::cmp::Ordering::Equal);
        self.near.sort_unstable_by(by_distance);
        self.far.sort_unstable_by(by_distance);
        self.patches.sort_unstable_by(|a, b| a.dist.partial_cmp(&b.dist).unwrap_or(core::cmp::Ordering::Equal));

        let world_state = |on: bool| {
            if on {
                sceGuEnable(GuState::Texture2D);
                sceGuEnable(GuState::Fragment2X);
                sceGuTexFunc(TextureEffect::Modulate, TextureColorComponent::Rgb);
                sceGuTexFilter(TextureFilter::LinearMipmapNearest, TextureFilter::Linear);
                sceGuTexLevelMode(TextureLevelMode::Auto, 0.0);
                sceGuTexWrap(GuTexWrapMode::Repeat, GuTexWrapMode::Clamp);
                sceGuTexScale(scene_h.u_range, 1.0);
                sceGuTexOffset(0.0, 0.0);
            } else {
                sceGuDisable(GuState::Fragment2X);
                sceGuDisable(GuState::Texture2D);
                sceGuTexFunc(TextureEffect::Modulate, TextureColorComponent::Rgba);
            }
        };
        sceGuEnable(GuState::DepthTest);
        sceGuEnable(GuState::Fog);
        sceGuFog(fog.0, fog.1, fog.2);
        if game.set.option & 1 == 0 {
            sceGuEnable(GuState::CullFace);
        }
        sceGuFrontFace(if game.set.option & 2 == 0 { FrontFaceDirection::CounterClockwise } else { FrontFaceDirection::Clockwise });
        sceGuDisable(GuState::Texture2D);

        // ------------------------------------------------------------ near pass
        // A knight beyond the near distance draws in the far pass: the far pass starts short of it.
        let split = lod_near;
        sceGuSetMatrix(MatrixMode::Projection, &near_proj);
        sceGuDepthRange(65535, DEPTH_SPLIT);
        let demon_at = {
            let (x, z, _) = game.sim.stage.demon;
            (game.sim.field.point(x, z) - cam.eye).len()
        };
        let demon = game.set.mage && !self.demon.batches.is_empty() && figures::demon_shown(&game.sim, &planes, cam.eye, 300.0);
        let lights = game.scene.figure_lights();
        if game.set.mage {
            self.figure_state(game, &lights, &cast, lit);
            let skin = game.sim.anim.skin;
            self.model(false, &skin);
            if demon && demon_at <= split {
                self.model(true, &requiem_sim::demon::skin(&game.sim));
            }
        }
        // Geometry with baked colours under the spells' lights: the constant term is white, so with no
        // light near, a vertex keeps its colour.
        if lit > 0 {
            sceGuEnable(GuState::Lighting);
            sceGuDisable(GuState::Light0);
            sceGuAmbient(0xffff_ffff);
            self.cast_lights(&cast, lit, false, &game.scene);
        } else {
            sceGuDisable(GuState::Lighting);
        }
        self.knights(true, split, fog);
        lap(1);
        let near_guard = Guard::new(&vp, scene_h.clip_near, 480.0, 272.0, 2048.0);
        world_state(true);
        for _ in 0..game.set.repeat {
            self.meshes(world, game, true, cam.eye, cam.look, &near_guard, scene_h.u_range);
        }
        world_state(false);
        sceGuDisable(GuState::Lighting);
        lap(2);

        // ------------------------------------------------------------ far pass
        sceGuSetMatrix(MatrixMode::Projection, &far_proj);
        sceGuDepthRange(DEPTH_SPLIT - 1, 0);
        let far_guard = Guard::new(&vp, far_near, 480.0, 272.0, 2048.0);
        world_state(true);
        for _ in 0..game.set.repeat {
            self.meshes(world, game, false, cam.eye, cam.look, &far_guard, scene_h.u_range);
        }
        world_state(false);
        self.knights(false, split, fog);
        // The ranks beyond the meshes' reach: every knight a figure of two quads, all in one draw.
        if game.set.crowd {
            let cap = requiem_handheld::crowd::FAR_FIGURES * requiem_handheld::crowd::FAR_VERTS;
            let mem = self.scratch.take((cap * core::mem::size_of::<ColorVertex>()) as i32) as *mut ColorVertex;
            let right = cam.look.cross(V3::UP).norm_or(v3(1.0, 0.0, 0.0));
            let n = self.list.far_figures(&game.sim, &planes, cam.eye, right, game.lod_scale, FAR_REACH * game.lod_scale, core::slice::from_raw_parts_mut(mem, cap));
            sceGuDisable(GuState::CullFace);
            if n > 0 {
                flush(mem, n as usize * requiem_handheld::crowd::FAR_VERTS * core::mem::size_of::<ColorVertex>());
                sceGuSetMatrix(MatrixMode::Model, &fmatrix(&mat::IDENTITY));
                sceGuDrawArray(GuPrimitive::Triangles, vtype_color(), (n * 12) as i32, self.quad_ib.as_ptr().cast(), mem.cast());
                self.stats.draws += 1;
                self.stats.tris += n * 4;
            }
            // The cohorts wholly beyond the meshes: each one's ranks as a mesh that is already written.
            self.ranks.pick(&game.sim, &planes, cam.eye, self.list.far * game.lod_scale, FAR_REACH * game.lod_scale, 4);
            for d in &self.ranks.draws {
                let vtx = self.ranks.vertices().add(d.first as usize);
                if d.built != 0 {
                    flush(vtx, d.verts as usize * core::mem::size_of::<ColorVertex>());
                }
                sceGuSetMatrix(MatrixMode::Model, &fmatrix(&mat::translated(&mat::IDENTITY, v3(d.offset[0], d.offset[1], d.offset[2]))));
                sceGuDrawArray(GuPrimitive::Triangles, vtype_color(), (d.verts / 4 * 6) as i32, self.quad_ib.as_ptr().cast(), vtx.cast());
                self.stats.draws += 1;
                self.stats.tris += d.verts / 2;
            }
            game.info.crowd.far = n + self.ranks.knights;
            if game.set.option & 1 == 0 {
                sceGuEnable(GuState::CullFace);
            }
        }
        if demon && demon_at > split {
            self.figure_state(game, &lights, &cast, lit);
            self.model(true, &requiem_sim::demon::skin(&game.sim));
            sceGuDisable(GuState::Lighting);
        }
        lap(3);

        // ------------------------------------------------------------ sky
        // At the far end of the depth range, so it fills only what is still clear.
        sceGuDepthRange(0, 0);
        sceGuDisable(GuState::Fog);
        sceGuDisable(GuState::CullFace);
        sceGuDepthMask(1);
        let at_eye = fmatrix(&mat::translated(&mat::IDENTITY, cam.eye));
        sceGuSetMatrix(MatrixMode::Model, &at_eye);
        sceGuDrawArray(GuPrimitive::Triangles, vtype_color(), figures::SKY_INDICES as i32, self.sky_ib.as_ptr().cast(), self.sky_vb.as_ptr().cast());
        sceGuEnable(GuState::Blend);
        sceGuDrawArray(GuPrimitive::Triangles, vtype_color(), (figures::STARS * 6) as i32, self.quad_ib.as_ptr().cast(), self.star_vb.as_ptr().cast());
        sceGuDrawArray(GuPrimitive::Triangles, vtype_color(), figures::MOON_INDICES as i32, self.moon_ib.as_ptr().cast(), self.moon_vb.as_ptr().cast());
        self.stats.draws += 3;
        self.stats.tris += (figures::SKY_INDICES / 3 + figures::STARS * 2 + figures::MOON_INDICES / 3) as u32;
        lap(4);

        // ------------------------------------------------------------ shadows and effects, with the near pass's frustum
        sceGuSetMatrix(MatrixMode::Projection, &near_proj);
        sceGuDepthRange(65535, DEPTH_SPLIT);
        sceGuSetMatrix(MatrixMode::Model, &fmatrix(&mat::IDENTITY));
        if game.set.crowd || game.set.mage {
            let bytes = figures::DISC_VERTS * core::mem::size_of::<ColorVertex>();
            let mem = self.scratch.take(bytes as i32) as *mut ColorVertex;
            let discs = figures::shadows(&game.sim, cam.eye, 22.0, core::slice::from_raw_parts_mut(mem, figures::DISC_VERTS));
            if discs > 0 {
                flush(mem, discs as usize * (figures::FAN + 1) * core::mem::size_of::<ColorVertex>());
                sceGuDrawArray(GuPrimitive::Triangles, vtype_color(), (discs as usize * figures::FAN * 3) as i32, self.fan_ib.as_ptr().cast(), mem.cast());
                self.stats.draws += 1;
                self.stats.tris += discs * figures::FAN as u32;
            }
        }
        if game.set.fx {
            let fv = self.scratch.take((FX_VERTS * core::mem::size_of::<FxVertex>()) as i32) as *mut FxVertex;
            let fi = self.scratch.take((FX_INDICES * 2) as i32) as *mut u16;
            let right = cam.look.cross(V3::UP).norm_or(v3(1.0, 0.0, 0.0));
            let fxcam = fx::Camera { eye: cam.eye, right, up: right.cross(cam.look), time: game.sim.tick as f32 * (1.0 / 60.0) };
            let b = self.fx.build(&game.sim, &fxcam, core::slice::from_raw_parts_mut(fv, FX_VERTS), core::slice::from_raw_parts_mut(fi, FX_INDICES));
            game.info.fx = b;
            if b.over + b.add > 0 {
                flush(fv, b.verts as usize * core::mem::size_of::<FxVertex>());
                flush(fi, (b.over + b.add) as usize * 2);
                sceGuEnable(GuState::Texture2D);
                sceGuClutMode(ClutPixelFormat::Psm8888, 0, 0xff, 0);
                sceGuClutLoad(32, ptr::addr_of!(RAMP.0) as *const c_void);
                sceGuTexMode(TexturePixelFormat::PsmT8, 0, 0, 1);
                sceGuTexImage(MipmapLevel::None, self.fx_side, self.fx_side, self.fx_side, self.fx_tex.cast());
                sceGuTexFunc(TextureEffect::Modulate, TextureColorComponent::Rgba);
                sceGuTexFilter(TextureFilter::Linear, TextureFilter::Linear);
                sceGuTexWrap(GuTexWrapMode::Clamp, GuTexWrapMode::Clamp);
                sceGuTexScale(1.0, 1.0);
                sceGuTexOffset(0.0, 0.0);
                if b.over > 0 {
                    sceGuDrawArray(GuPrimitive::Triangles, vtype_fx(), b.over as i32, fi.cast(), fv.cast());
                }
                if b.add > 0 {
                    sceGuBlendFunc(BlendOp::Add, BlendFactor::SrcAlpha, BlendFactor::Fix, 0, 0x00ff_ffff);
                    sceGuDrawArray(GuPrimitive::Triangles, vtype_fx(), b.add as i32, fi.add(b.over as usize).cast(), fv.cast());
                    sceGuBlendFunc(BlendOp::Add, BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha, 0, 0);
                }
                self.stats.draws += 2;
                self.stats.tris += (b.over + b.add) / 3;
            }
        }
        sceGuDepthMask(0);
        lap(5);

        // ------------------------------------------------------------ interface
        sceGuDisable(GuState::DepthTest);
        sceGuEnable(GuState::Texture2D);
        sceGuTexMode(TexturePixelFormat::Psm4444, 0, 0, 1);
        sceGuTexImage(MipmapLevel::None, self.font.width as i32, self.font.height as i32, self.font.width as i32, self.font_tex.cast());
        sceGuTexFunc(TextureEffect::Modulate, TextureColorComponent::Rgba);
        sceGuTexFilter(TextureFilter::Nearest, TextureFilter::Nearest);
        sceGuTexWrap(GuTexWrapMode::Clamp, GuTexWrapMode::Clamp);
        let hv = self.scratch.take((MAX_QUADS * 4 * core::mem::size_of::<HudVertex>()) as i32) as *mut HudVertex;
        let quads = {
            let mut p = *perf;
            p.draws = self.stats.draws;
            p.tris = self.stats.tris;
            game.measure(&p);
            let mut hud = Hud::new(&self.font, core::slice::from_raw_parts_mut(hv, MAX_QUADS * 4), [1.0, 1.0], [0.0, 0.0]);
            game.draw_hud(&mut hud);
            hud.quads
        };
        if quads > 0 {
            flush(hv, quads * 4 * core::mem::size_of::<HudVertex>());
            sceGuDrawArray(GuPrimitive::Triangles, vtype_hud(), (quads * 6) as i32, self.quad_ib.as_ptr().cast(), hv.cast());
            self.stats.draws += 1;
        }
        sceGuDisable(GuState::Blend);
        sceGuFinish();
        lap(6);
        self.stats.phase = phase;
        self.stats.pick_world = pick_world;
    }

    /// The state the mage and the demon draw with: the moon as a directional light over a constant, and the spells' lights.
    unsafe fn figure_state(&self, game: &Game, lights: &scene::Lights, cast: &[Cast; LIGHTS], lit: usize) {
        sceGuEnable(GuState::Lighting);
        sceGuEnable(GuState::Light0);
        let d = game.scene.sun_dir;
        sceGuLight(0, LightType::Directional, LightComponent::DIFFUSE, &ScePspFVector3 { x: d.x, y: d.y, z: d.z });
        sceGuLightColor(0, LightComponent::DIFFUSE, abgr(lights.moon));
        sceGuAmbient(abgr(lights.ambient));
        self.cast_lights(cast, lit, true, &game.scene);
        sceGuSetMatrix(MatrixMode::Model, &fmatrix(&mat::IDENTITY));
    }
}
