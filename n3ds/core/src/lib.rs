//! C interface of the 3DS host (`../src/core.h` declares the same functions).
//!
//! The host owns the GPU, the pad, sound and storage; it hands the pack's
//! sections in once and then asks, each frame, what to draw.

#![no_std]

extern crate alloc;

#[path = "alloc.rs"]
mod allocator;

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_char;

use requiem_handheld::crowd::{self, CrowdList, Knight, RankDraw, Ranks};
use requiem_handheld::figures::{self, ColorVertex};
use requiem_handheld::fx::{self, Batches, Fx, FxVertex};
use requiem_handheld::game::{Game, Info, Pad, Perf};
use requiem_handheld::ground::{self, Grid, Ground, Layout, Patch};
use requiem_handheld::hud::{Font, Hud, HudVertex};
use requiem_handheld::mat::{self, Mat4};
use requiem_handheld::scene::{self, Cast, Scene};
use requiem_handheld::world::{Pick, World};
use requiem_pack::{self as pack, CrowdHeader, CrowdMesh, HandMesh, HandScene, PicaVertex};
use requiem_sim::fx::LIGHTS;
use requiem_sim::math::*;
use requiem_sim::skel::BONES;

const HELP: &str = "Y strike   X spell   B evade   A unseal   L guard   R hover";
/// Knights are drawn as far figures out to this distance.
const FAR_REACH: f32 = 420.0;

struct App {
    game: Game,
    world: World,
    font: Font,
    fx: Fx,
    list: CrowdList,
    ground: Option<Ground>,
    ranks: Option<Ranks>,
    far: Vec<Pick>,
    near: Vec<Pick>,
    patches: Vec<Patch>,
    vp: Mat4,
    planes: [[f32; 4]; 6],
    eye: V3,
    look: V3,
    cast: [Cast; LIGHTS],
    demon: bool,
    text: String,
}

static mut APP: Option<App> = None;

fn app() -> &'static mut App {
    // The host calls from one thread, after `rq_init` succeeded.
    unsafe { (*core::ptr::addr_of_mut!(APP)).as_mut().unwrap_unchecked() }
}

/// Sizes the host's buffers must have; `rq_sizes` returns them so a mismatch with `core.h` fails at start.
#[repr(C)]
pub struct RqSizes {
    pub bones: u32,
    pub sky_verts: u32,
    pub sky_indices: u32,
    pub star_verts: u32,
    pub moon_verts: u32,
    pub moon_indices: u32,
    pub disc_verts: u32,
    pub disc_indices: u32,
    pub fan: u32,
    pub far_verts: u32,
    pub far_figures: u32,
    pub rank_verts: u32,
    pub ground_near_indices: u32,
    pub ground_small_indices: u32,
    pub scene_bytes: u32,
    pub mesh_bytes: u32,
    pub vertex_bytes: u32,
    pub knight_bytes: u32,
    pub patch_bytes: u32,
    pub view_bytes: u32,
}

#[no_mangle]
pub extern "C" fn rq_sizes(out: *mut RqSizes) {
    unsafe {
        *out = RqSizes {
            bones: BONES as u32,
            sky_verts: figures::SKY_VERTS as u32,
            sky_indices: figures::SKY_INDICES as u32,
            star_verts: figures::STAR_VERTS as u32,
            moon_verts: figures::MOON_VERTS as u32,
            moon_indices: figures::MOON_INDICES as u32,
            disc_verts: figures::DISC_VERTS as u32,
            disc_indices: figures::DISC_INDICES as u32,
            fan: figures::FAN as u32,
            far_verts: crowd::FAR_VERTS as u32,
            far_figures: crowd::FAR_FIGURES as u32,
            rank_verts: crowd::RANK_VERTS as u32,
            ground_near_indices: ground::index_count(ground::NEAR_N) as u32,
            ground_small_indices: ground::index_count(ground::SMALL_N) as u32,
            scene_bytes: core::mem::size_of::<HandScene>() as u32,
            mesh_bytes: core::mem::size_of::<HandMesh>() as u32,
            vertex_bytes: core::mem::size_of::<PicaVertex>() as u32,
            knight_bytes: core::mem::size_of::<Knight>() as u32,
            patch_bytes: core::mem::size_of::<Patch>() as u32,
            view_bytes: core::mem::size_of::<RqView>() as u32,
        };
    }
}

fn fail(text: &'static str) -> *const c_char {
    // Every message below ends in a NUL.
    text.as_ptr().cast()
}

/// The pack's sections, as the host read them. The host may free all of them after `rq_init` except `crowd`,
/// which it keeps in memory the GPU reads.
#[repr(C)]
pub struct RqPack {
    pub scene: *const HandScene,
    pub meshes: *const HandMesh,
    pub mesh_count: u32,
    pub simw: *const u8,
    pub simw_len: u32,
    pub font: *const u8,
    pub font_len: u32,
    pub fxpk: *const u8,
    pub fxpk_len: u32,
    pub grnd: *const u8,
    pub grnd_len: u32,
    pub crowd: *const u8,
    pub crowd_len: u32,
}

/// Bytes of linear memory the ground's patches and the army's far ranks take: the host allocates them
/// and hands them to `rq_memory`.
#[repr(C)]
pub struct RqMemory {
    pub ground: u32,
    pub ranks: u32,
}

/// Builds the game from the pack's sections. Returns null, or a message.
///
/// # Safety
/// The pointers are valid for their lengths.
#[no_mangle]
pub unsafe extern "C" fn rq_init(p: *const RqPack, need: *mut RqMemory) -> *const c_char {
    let p = &*p;
    let scene = Scene::new(core::ptr::read_unaligned(p.scene));
    let recs: Vec<HandMesh> = (0..p.mesh_count as usize).map(|i| core::ptr::read_unaligned(p.meshes.add(i))).collect();
    let per = ((scene.h.super_cell / scene.h.cell + 0.5) as i32).max(1);
    let world = World::new(recs, per, false, core::mem::size_of::<PicaVertex>() as u32);
    let sim = match requiem_sim::worldfile::load(core::slice::from_raw_parts(p.simw, p.simw_len as usize)) {
        Ok(s) => s,
        Err(_) => return fail("the pack's stage does not load\0"),
    };
    let font = match Font::parse(core::slice::from_raw_parts(p.font, p.font_len as usize)) {
        Ok((f, _)) => f,
        Err(_) => return fail("the pack's font does not load\0"),
    };
    let fx = match Fx::parse(core::slice::from_raw_parts(p.fxpk, p.fxpk_len as usize)) {
        Ok(f) => f,
        Err(_) => return fail("the pack's effects do not load\0"),
    };
    let crowd = core::slice::from_raw_parts(p.crowd, p.crowd_len as usize);
    let Some(head) = pack::read::<CrowdHeader>(crowd, 0) else { return fail("the pack's army does not load\0") };
    let tris: Vec<u32> = (0..(head.kinds * head.lods) as usize).filter_map(|i| pack::read::<CrowdMesh>(crowd, core::mem::size_of::<CrowdHeader>() + i * core::mem::size_of::<CrowdMesh>())).map(|m| m.idx_count / 3).collect();
    if head.lods != scene.h.crowd_lods || head.kinds != 3 || tris.len() != (head.kinds * head.lods) as usize {
        return fail("the pack's army does not match its scene\0");
    }
    let list = CrowdList::new(&scene.h, tris, false);
    *need = RqMemory { ground: Ground::bytes(Layout::Pica) as u32, ranks: Ranks::bytes(&sim) as u32 };
    let grid = match Grid::parse(core::slice::from_raw_parts(p.grnd, p.grnd_len as usize)) {
        Ok(g) => g,
        Err(_) => return fail("the pack's ground does not load\0"),
    };
    // The ground and the ranks are finished by `rq_memory`; until then the grid waits here.
    PENDING = Some(grid);
    let game = Game::new(sim, scene, HELP);
    APP = Some(App {
        game,
        world,
        font,
        fx,
        list,
        ground: None,
        ranks: None,
        far: Vec::with_capacity(1024),
        near: Vec::with_capacity(256),
        patches: Vec::with_capacity(160),
        vp: mat::IDENTITY,
        planes: [[0.0; 4]; 6],
        eye: V3::ZERO,
        look: v3(0.0, 0.0, -1.0),
        cast: [Cast::default(); LIGHTS],
        demon: false,
        text: String::with_capacity(4096),
    });
    core::ptr::null()
}

static mut PENDING: Option<Grid> = None;

/// Hands over the linear memory `rq_init` asked for. Returns null, or a message.
///
/// # Safety
/// The two blocks are as large as `RqMemory` said, and stay for the life of the program.
#[no_mangle]
pub unsafe extern "C" fn rq_memory(ground_mem: *mut u8, ranks_mem: *mut u8) -> *const c_char {
    let a = app();
    let Some(grid) = (*core::ptr::addr_of_mut!(PENDING)).take() else { return fail("rq_memory before rq_init\0") };
    let h = a.game.scene.h;
    a.ground = match Ground::new(grid, &a.game.sim.field, Layout::Pica, h.cell, h.super_cell, h.u_range, ground_mem) {
        Ok(g) => Some(g),
        Err(_) => return fail("the ground's grid does not match the simulation's\0"),
    };
    a.ranks = Some(Ranks::new(&a.game.sim, ranks_mem, a.list.tones));
    core::ptr::null()
}

/// Offset of the texture bytes in the `FONT` section, and the atlas size.
#[no_mangle]
pub unsafe extern "C" fn rq_font(font: *const u8, font_len: u32, width: *mut u32, height: *mut u32) -> u32 {
    match Font::parse(core::slice::from_raw_parts(font, font_len as usize)) {
        Ok((f, at)) => {
            *width = f.width;
            *height = f.height;
            at as u32
        }
        Err(_) => 0,
    }
}

/// Static geometry: the sky dome, the stars, the moon, and the index lists.
/// `pixel` is the radians one pixel covers.
#[no_mangle]
pub unsafe extern "C" fn rq_static(radius: f32, pixel: f32, sky_v: *mut ColorVertex, sky_i: *mut u16, star_v: *mut ColorVertex, moon_v: *mut ColorVertex, moon_i: *mut u16, quad_i: *mut u16, quads: u32, fan_i: *mut u16, ground_near_i: *mut u16, ground_small_i: *mut u16) {
    let a = app();
    figures::sky(&a.game.scene, radius, core::slice::from_raw_parts_mut(sky_v, figures::SKY_VERTS), core::slice::from_raw_parts_mut(sky_i, figures::SKY_INDICES));
    figures::stars(radius * 0.96, pixel, core::slice::from_raw_parts_mut(star_v, figures::STAR_VERTS));
    figures::moon(&a.game.scene, radius * 0.94, core::slice::from_raw_parts_mut(moon_v, figures::MOON_VERTS), core::slice::from_raw_parts_mut(moon_i, figures::MOON_INDICES));
    figures::quad_indices(core::slice::from_raw_parts_mut(quad_i, quads as usize * 6), quads as usize);
    figures::fan_indices(core::slice::from_raw_parts_mut(fan_i, figures::DISC_INDICES), figures::DISCS, figures::FAN);
    ground::indices(ground::NEAR_N, core::slice::from_raw_parts_mut(ground_near_i, ground::index_count(ground::NEAR_N)));
    ground::indices(ground::SMALL_N, core::slice::from_raw_parts_mut(ground_small_i, ground::index_count(ground::SMALL_N)));
}

#[no_mangle]
pub unsafe extern "C" fn rq_control(text: *const u8, len: u32) {
    if let Ok(t) = core::str::from_utf8(core::slice::from_raw_parts(text, len as usize)) {
        app().game.control(t);
    }
}

#[no_mangle]
pub unsafe extern "C" fn rq_step(pad: *const Pad, ticks: u32) -> u32 {
    app().game.step(&*pad, ticks)
}

/// The frame's camera, switches and counts.
#[repr(C)]
pub struct RqView {
    pub eye: [f32; 3],
    pub fov: f32,
    pub look: [f32; 3],
    pub roll: f32,
    pub fog: [f32; 3],
    /// How much of a heavy strike's freeze is left, 1 to 0.
    pub impact: f32,
    /// The moon's direction and how much of it arrives, its colour, sky, bounce.
    pub light: [f32; 16],
    /// The strongest lights the spells cast.
    pub cast: [Cast; LIGHTS],
    pub cast_count: u32,
    pub world: u32,
    pub mage: u32,
    pub demon: u32,
    pub fx: u32,
    pub repeat: u32,
    pub option: i32,
    pub far_count: u32,
    pub near_count: u32,
    pub patch_count: u32,
    pub knight_count: u32,
    pub rank_count: u32,
}

/// Chooses this frame's camera, props, ground patches, knights and far ranks.
#[no_mangle]
pub unsafe extern "C" fn rq_view(out: *mut RqView) {
    let a = app();
    let cam = a.game.camera();
    a.vp = a.game.view_proj(&cam);
    a.planes = mat::planes(&a.vp);
    a.eye = cam.eye;
    a.look = cam.look;
    a.far.clear();
    a.near.clear();
    a.patches.clear();
    a.game.info = Info::default();
    let (near, mid, far) = a.game.lod();
    if a.game.set.world {
        let picked = a.world.pick(&a.planes, cam.eye, near, mid, far, &|_| true, &mut a.far, &mut a.near);
        a.game.info.props = picked.near + picked.mid + picked.far;
        if let Some(g) = a.ground.as_mut() {
            g.pick(&a.game.sim.field, &a.planes, cam.eye, near, mid, far, a.game.frame, &mut a.patches);
            a.game.info.patches = a.patches.len() as u32;
            a.game.info.built = g.built;
        }
    }
    // In the pack's order, meshes that share a vertex base are adjacent: the host merges them into runs.
    a.far.sort_unstable_by_key(|p| p.mesh);
    a.near.sort_unstable_by_key(|p| p.mesh);
    let mut ranks = 0;
    if a.game.set.crowd {
        a.game.info.crowd = a.list.build(&a.game.sim, &a.planes, cam.eye, a.game.lod_scale);
        if let Some(r) = a.ranks.as_mut() {
            r.pick(&a.game.sim, &a.planes, cam.eye, a.list.far * a.game.lod_scale, FAR_REACH * a.game.lod_scale, 4);
            a.game.info.crowd.far = r.knights;
            ranks = r.draws.len() as u32;
        }
    } else {
        a.list.out.clear();
    }
    let lit = scene::cast(&a.game.sim, cam.eye, &mut a.cast);
    a.demon = a.game.set.mage && figures::demon_shown(&a.game.sim, &a.planes, cam.eye, 300.0);
    let set = &a.game.set;
    *out = RqView {
        eye: [cam.eye.x, cam.eye.y, cam.eye.z],
        fov: cam.fov,
        look: [cam.look.x, cam.look.y, cam.look.z],
        roll: cam.roll,
        fog: a.game.scene.fog_srgb(),
        impact: a.game.impact(),
        light: a.game.scene.light(1.0),
        cast: a.cast,
        cast_count: lit as u32,
        world: set.world as u32,
        mage: set.mage as u32,
        demon: a.demon as u32,
        fx: set.fx as u32,
        repeat: set.repeat,
        option: set.option,
        far_count: a.far.len() as u32,
        near_count: a.near.len() as u32,
        patch_count: a.patches.len() as u32,
        knight_count: a.list.out.len() as u32,
        rank_count: ranks,
    };
}

/// The props of `rq_view`: list 0 is beyond the near distance, list 1 within it.
#[no_mangle]
pub extern "C" fn rq_picks(list: u32) -> *const Pick {
    let a = app();
    if list == 0 {
        a.far.as_ptr()
    } else {
        a.near.as_ptr()
    }
}

#[no_mangle]
pub extern "C" fn rq_patches() -> *const Patch {
    app().patches.as_ptr()
}

/// The knights of `rq_view`, sorted so the ones that share a mesh and a frame follow each other.
#[no_mangle]
pub extern "C" fn rq_knights() -> *const Knight {
    app().list.out.as_ptr()
}

#[no_mangle]
pub extern "C" fn rq_ranks() -> *const RankDraw {
    match app().ranks.as_ref() {
        Some(r) => r.draws.as_ptr(),
        None => core::ptr::null(),
    }
}

/// The knights beyond the meshes that are in no cohort drawn whole, as figures: `FAR_VERTS` vertices each.
#[no_mangle]
pub unsafe extern "C" fn rq_far_figures(verts: *mut ColorVertex, cap: u32) -> u32 {
    let a = app();
    if !a.game.set.crowd {
        return 0;
    }
    let right = a.look.cross(V3::UP).norm_or(v3(1.0, 0.0, 0.0));
    let n = a.list.far_figures(&a.game.sim, &a.planes, a.eye, right, a.game.lod_scale, FAR_REACH * a.game.lod_scale, core::slice::from_raw_parts_mut(verts, cap as usize));
    a.game.info.crowd.far += n;
    n
}

/// The shadows on the ground: `FAN + 1` vertices each.
#[no_mangle]
pub unsafe extern "C" fn rq_shadows(verts: *mut ColorVertex, cap: u32) -> u32 {
    let a = app();
    figures::shadows(&a.game.sim, a.eye, 24.0, core::slice::from_raw_parts_mut(verts, cap as usize))
}

/// Evaluates the live effects into vertices and indices.
#[no_mangle]
pub unsafe extern "C" fn rq_fx(verts: *mut FxVertex, vert_cap: u32, idx: *mut u16, idx_cap: u32, out: *mut Batches) {
    let a = app();
    let right = a.look.cross(V3::UP).norm_or(v3(1.0, 0.0, 0.0));
    let cam = fx::Camera { eye: a.eye, right, up: right.cross(a.look), time: a.game.sim.tick as f32 * (1.0 / 60.0) };
    let b = a.fx.build(&a.game.sim, &cam, core::slice::from_raw_parts_mut(verts, vert_cap as usize), core::slice::from_raw_parts_mut(idx, idx_cap as usize));
    a.game.info.fx = b;
    *out = b;
}

/// Three rows per bone of one draw of the mage (`demon` 0) or the demon (1): `count` bones, 12 floats each.
#[no_mangle]
pub unsafe extern "C" fn rq_bones(demon: u32, bones: *const u8, count: u32, rows: *mut f32) {
    let a = app();
    let bones = core::slice::from_raw_parts(bones, count as usize);
    let out = core::slice::from_raw_parts_mut(rows, count as usize * 12);
    if demon != 0 {
        figures::bone_rows(&requiem_sim::demon::skin(&a.game.sim), bones, out);
    } else {
        figures::bone_rows(&a.game.sim.anim.skin, bones, out);
    }
}

/// Batches the interface into `verts` (four per quad). Texture coordinates leave as `texel × scale + offset`.
#[no_mangle]
pub unsafe extern "C" fn rq_hud(verts: *mut HudVertex, cap: u32, uv_scale: *const f32, uv_offset: *const f32, perf: *const Perf) -> u32 {
    let a = app();
    a.game.measure(&*perf);
    let mut hud = Hud::new(&a.font, core::slice::from_raw_parts_mut(verts, cap as usize), [*uv_scale, *uv_scale.add(1)], [*uv_offset, *uv_offset.add(1)]);
    a.game.draw_hud(&mut hud);
    hud.quads as u32
}

#[no_mangle]
pub unsafe extern "C" fn rq_audio(out: *mut i16, frames: u32, rate: f32) {
    app().game.synth.render(core::slice::from_raw_parts_mut(out, frames as usize * 2), rate);
}

/// The status record as JSON, NUL-terminated. `extra` is the host's members without braces.
#[no_mangle]
pub unsafe extern "C" fn rq_status(out: *mut u8, cap: u32, perf: *const Perf, extra: *const u8, extra_len: u32) -> u32 {
    let a = app();
    a.text.clear();
    let extra = core::str::from_utf8(core::slice::from_raw_parts(extra, extra_len as usize)).unwrap_or("");
    let mut text = core::mem::take(&mut a.text);
    a.game.status(&mut text, "3ds", &*perf, extra);
    let n = text.len().min(cap as usize - 1);
    core::ptr::copy_nonoverlapping(text.as_ptr(), out, n);
    *out.add(n) = 0;
    a.text = text;
    n as u32
}

/// For the lower screen: the mage, the count, and where the army stands.
#[repr(C)]
pub struct RqMap {
    pub player: [f32; 3],
    pub yaw: f32,
    pub hp: f32,
    pub mana: f32,
    pub kos: u32,
    pub goal: u32,
    pub standing: u32,
    pub chain: u32,
    pub ticks: u32,
    pub auto_on: u32,
    pub won: u32,
    pub demon: [f32; 2],
}

/// Fills `out`; writes up to `cap` cohorts still in formation as x, z pairs and returns how many.
#[no_mangle]
pub unsafe extern "C" fn rq_map(out: *mut RqMap, cohort_xz: *mut f32, cap: u32) -> u32 {
    let g = &app().game;
    let s = &g.sim;
    *out = RqMap {
        player: [s.p.pos.x, s.p.pos.y, s.p.pos.z],
        yaw: s.cam.yaw,
        hp: s.p.hp / requiem_sim::sim::tune::HP_MAX,
        mana: s.p.mana / requiem_sim::sim::tune::MANA_MAX,
        kos: s.p.kos,
        goal: s.goal,
        standing: s.crowd.standing() as u32,
        chain: s.p.chain,
        ticks: s.tick,
        auto_on: g.set.auto as u32,
        won: s.won as u32,
        demon: [s.stage.demon.0, s.stage.demon.1],
    };
    let mut n = 0u32;
    for c in s.crowd.cohorts.iter().filter(|c| c.formed > 0) {
        if n >= cap {
            break;
        }
        *cohort_xz.add(n as usize * 2) = c.x;
        *cohort_xz.add(n as usize * 2 + 1) = c.z;
        n += 1;
    }
    n
}
