//! C interface for the wasm host, and the flat snapshot every host reads.
//!
//! A snapshot is one `f32` array. `layout!` assigns the offsets; the `abi`
//! binary prints them as TypeScript so the reference never hand-copies one.
//! The crowd, the effects and the bolts are read in place as arrays of
//! `repr(C)` records.

use alloc::vec::Vec;

use crate::crowd::Draw;
use crate::mage::{AnimIn, Animator};
use crate::fx::Light;
use crate::knight::Knight;
use crate::math::*;
use crate::sim::{Input, Sim};
use crate::skel::{Skeleton, BONES};

pub const ABI_VERSION: u32 = 1;

macro_rules! layout {
    ($( $name:ident : $n:expr ),* $(,)?) => {
        layout!(@step 0usize; $($name : $n,)*);
        /// Name, offset and length of every snapshot field.
        pub const FIELDS: &[(&str, usize, usize)] = &[ $( (stringify!($name), $name, $n) ),* ];
    };
    (@step $off:expr; $name:ident : $n:expr, $($rest:tt)*) => {
        pub const $name: usize = $off;
        layout!(@step $off + $n; $($rest)*);
    };
    (@step $off:expr;) => {
        pub const LEN: usize = $off;
    };
}

pub mod snap {
    use super::BONES;
    use crate::fx::LIGHTS as LIGHT_SLOTS;
    layout! {
        TICK: 1, POS: 3, VEL: 3, YAW: 1, ACT: 1, MOVE: 1, MOVE_T: 1, HP: 1, MANA: 1, CHAIN: 1, KOS: 1, GOAL: 1, STANDING: 1,
        STOP: 1, STOP_LEN: 1, HOVER: 1, EVENTS: 1, WON: 1, NEAR: 1, FREE: 1,
        CAM_POS: 3, CAM_LOOK: 3, CAM_FOV: 1, CAM_SHAKE: 1, CAM_YAW: 1, CAM_PITCH: 1,
        STAFF_HEAD: 3, STAFF_DIR: 3,
        LIGHT_COUNT: 1, LIGHTS: LIGHT_SLOTS * 8,
        // Skin matrices: three columns of rotation, then translation.
        SKIN: BONES * 12,
    }
}

fn put(out: &mut [f32], at: usize, v: V3) {
    out[at] = v.x;
    out[at + 1] = v.y;
    out[at + 2] = v.z;
}

impl Sim {
    /// Writes the snapshot; `out` must hold `snap::LEN` floats.
    pub fn snapshot(&self, out: &mut [f32]) {
        use snap::*;
        let p = &self.p;
        out[TICK] = self.tick as f32;
        put(out, POS, p.pos);
        put(out, VEL, p.vel);
        out[YAW] = p.yaw;
        out[ACT] = p.act as f32;
        out[MOVE] = if p.act == crate::sim::act::MOVE { p.mv as f32 } else { 0.0 };
        out[MOVE_T] = p.t as f32;
        out[HP] = p.hp;
        out[MANA] = p.mana;
        out[CHAIN] = p.chain as f32;
        out[KOS] = p.kos as f32;
        out[GOAL] = self.goal as f32;
        out[STANDING] = self.crowd.standing() as f32;
        out[STOP] = self.stop as f32;
        out[STOP_LEN] = self.stop_len as f32;
        out[HOVER] = p.hover;
        out[EVENTS] = self.events as f32;
        out[WON] = self.won as u32 as f32;
        out[NEAR] = self.crowd.near as f32;
        out[FREE] = self.crowd.free.len() as f32;
        put(out, CAM_POS, self.cam.pos);
        put(out, CAM_LOOK, self.cam.look);
        out[CAM_FOV] = self.cam.fov;
        out[CAM_SHAKE] = self.cam.shake;
        out[CAM_YAW] = self.cam.yaw;
        out[CAM_PITCH] = self.cam.pitch;
        put(out, STAFF_HEAD, self.anim.head);
        put(out, STAFF_DIR, self.anim.staff);
        let mut lights = [Light::default(); crate::fx::LIGHTS];
        let n = self.fx.lights(self.tick, self.cam.pos, &mut lights);
        out[LIGHT_COUNT] = n as f32;
        for (k, l) in lights.iter().enumerate() {
            let at = snap::LIGHTS + k * 8;
            let on = if k < n { l.power } else { 0.0 };
            put(out, at, l.pos);
            out[at + 3] = l.radius;
            out[at + 4] = l.color[0] * on;
            out[at + 5] = l.color[1] * on;
            out[at + 6] = l.color[2] * on;
            out[at + 7] = on;
        }
        put_skin(&mut out[SKIN..SKIN + BONES * 12], &self.anim.skin);
    }
}

/// Twelve floats per bone: the rotation's three columns, then the translation.
pub fn put_skin(out: &mut [f32], skin: &[M34; BONES]) {
    for (i, m) in skin.iter().enumerate() {
        let at = i * 12;
        put(out, at, m.r.x);
        put(out, at + 3, m.r.y);
        put(out, at + 6, m.r.z);
        put(out, at + 9, m.t);
    }
}

// ---------------------------------------------------------------------- wasm

static mut SIM: Option<Sim> = None;
static mut SNAP: [f32; snap::LEN] = [0.0; snap::LEN];
static mut MATS: [f32; BONES * 12] = [0.0; BONES * 12];
static mut DRAWS: Vec<Draw> = Vec::new();
static mut PLANES: [f32; 24] = [0.0; 24];
static mut KNIGHTS: Vec<Knight> = Vec::new();
static mut PREVIEW: Option<Animator> = None;
static mut SYNTH: Option<crate::audio::Synth> = None;
static mut PCM: [i16; 8192] = [0; 8192];

#[allow(static_mut_refs)]
fn sim() -> Option<&'static mut Sim> {
    unsafe { SIM.as_mut() }
}

#[no_mangle]
pub extern "C" fn rq_abi_version() -> u32 {
    ABI_VERSION
}

#[no_mangle]
pub extern "C" fn rq_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    core::mem::forget(v);
    p
}

/// # Safety
/// `ptr` and `len` must come from `rq_alloc`.
#[no_mangle]
pub unsafe extern "C" fn rq_free(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, 0, len.max(1)));
}

/// Loads a world file. Returns 0, or a negative code.
///
/// # Safety
/// `ptr` must point at `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn rq_load(ptr: *const u8, len: usize) -> i32 {
    match crate::worldfile::load(core::slice::from_raw_parts(ptr, len)) {
        Ok(s) => {
            SIM = Some(s);
            0
        }
        Err(_) => -1,
    }
}

/// A flat stage with `cohorts` blocks of knights, for previews.
#[no_mangle]
pub extern "C" fn rq_load_test(cohorts: u32) {
    let (field, stage) = crate::worldfile::test_stage(cohorts as usize);
    unsafe { SIM = Some(Sim::new(field, stage)) };
}

#[no_mangle]
pub extern "C" fn rq_reset() {
    if let Some(s) = sim() {
        s.reset();
    }
}

#[no_mangle]
pub extern "C" fn rq_tick(buttons: u32, lx: f32, ly: f32, rx: f32, ry: f32) {
    if let Some(s) = sim() {
        s.tick(Input { buttons, lx, ly, rx, ry });
        listen(s);
    }
}

#[allow(static_mut_refs)]
fn listen(s: &Sim) {
    unsafe { SYNTH.get_or_insert_with(crate::audio::Synth::new).control(s, s.events) }
}

/// One tick played by the autopilot. Returns the buttons it pressed.
#[no_mangle]
pub extern "C" fn rq_tick_auto() -> u32 {
    match sim() {
        Some(s) => {
            let i = s.auto_input();
            s.tick(i);
            listen(s);
            i.buttons
        }
        None => 0,
    }
}

/// Renders `frames` stereo frames (at most 4096) at `rate` Hz; returns interleaved 16-bit samples.
#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn rq_audio(frames: u32, rate: f32) -> *const i16 {
    unsafe {
        let n = (frames as usize).min(4096) * 2;
        SYNTH.get_or_insert_with(crate::audio::Synth::new).render(&mut PCM[..n], rate);
        PCM.as_ptr()
    }
}

#[no_mangle]
pub extern "C" fn rq_snapshot_len() -> u32 {
    snap::LEN as u32
}

#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn rq_snapshot() -> *const f32 {
    unsafe {
        if let Some(s) = SIM.as_ref() {
            s.snapshot(&mut SNAP);
        }
        SNAP.as_ptr()
    }
}

/// Where the host writes six frustum planes (`n·p + d >= 0` inside) before `rq_crowd`.
#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn rq_planes() -> *mut f32 {
    unsafe { PLANES.as_mut_ptr() }
}

/// Collects the knights inside the planes and within `far` of the eye. Returns how many; `rq_crowd_ptr` points at them.
#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn rq_crowd(ex: f32, ey: f32, ez: f32, far: f32) -> u32 {
    unsafe {
        let Some(s) = SIM.as_ref() else { return 0 };
        let mut planes = [[0.0f32; 4]; 6];
        for (k, p) in planes.iter_mut().enumerate() {
            p.copy_from_slice(&PLANES[k * 4..k * 4 + 4]);
        }
        s.crowd.draw(&s.field, s.tick, &planes, v3(ex, ey, ez), far, &mut DRAWS);
        DRAWS.len() as u32
    }
}

#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn rq_crowd_ptr() -> *const Draw {
    unsafe { DRAWS.as_ptr() }
}

/// The effect slots (`fx::SLOTS` records of `fx::Fx`).
#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn rq_fx() -> *const crate::fx::Fx {
    unsafe { SIM.as_ref().map_or(core::ptr::null(), |s| s.fx.items.as_ptr()) }
}

/// The bolts (`sim::BOLTS` records of `sim::Bolt`).
#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn rq_bolts() -> *const crate::sim::Bolt {
    unsafe { SIM.as_ref().map_or(core::ptr::null(), |s| s.bolts.as_ptr()) }
}

#[no_mangle]
pub extern "C" fn rq_height(x: f32, z: f32) -> f32 {
    sim().map_or(0.0, |s| s.field.height(x, z))
}

/// Bind-pose bone transforms a figure's model is built on (`skel::figure`).
#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn rq_bind(kind: u32) -> *const f32 {
    unsafe {
        put_skin(&mut MATS, &Skeleton::of(kind).bind());
        MATS.as_ptr()
    }
}

#[allow(static_mut_refs)]
fn knight(kind: u32) -> &'static Knight {
    unsafe {
        if KNIGHTS.is_empty() {
            for k in 1..=3 {
                KNIGHTS.push(Knight::new(k));
            }
        }
        &KNIGHTS[(kind.clamp(1, 3) - 1) as usize]
    }
}

/// Skin matrices of a knight (`figure` 1 to 3) at stored frame `frame`, in the figure's frame.
#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn rq_knight_frame(kind: u32, frame: u32) -> *const f32 {
    let k = knight(kind);
    let world = k.frame(frame as u16);
    unsafe {
        put_skin(&mut MATS, &crate::anim::skin(&world, &k.skel.bind_inverse()));
        MATS.as_ptr()
    }
}

/// Skin matrices of the mage standing at the origin in a given state, for model previews:
/// `show` and `mv` as the animator takes them, `t` ticks into it.
#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn rq_mage_pose(show: u32, mv: u32, t: f32, phase: f32, speed: f32, hover: f32) -> *const f32 {
    unsafe {
        let a = PREVIEW.get_or_insert_with(Animator::new);
        // Twice: the first call starts the cross-fade from whatever was shown, the second lands on the pose.
        a.reset();
        let input = AnimIn { pos: V3::ZERO, yaw: 0.0, speed, show: show as u8, mv: mv as u8, t, phase, hover, tick: (t as u32).wrapping_mul(1), frozen: false };
        a.update(&input);
        for _ in 0..12 {
            a.update(&input);
        }
        put_skin(&mut MATS, &a.skin);
        MATS.as_ptr()
    }
}

/// Skin matrices of the demon where she stands.
#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn rq_demon() -> *const f32 {
    unsafe {
        if let Some(s) = SIM.as_ref() {
            put_skin(&mut MATS, &crate::demon::skin(s));
        }
        MATS.as_ptr()
    }
}
