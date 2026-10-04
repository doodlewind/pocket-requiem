//! The headless knights' motion: eleven clips per kind of knight, each a
//! function from time to a pose.
//!
//! The crowd never runs this at play time. The world compiler samples every
//! clip at the frame times of `CLIPS`, skins each level of detail at those
//! frames and stores the results as vertex streams; a device draws a knight
//! as a blend of two stored frames. The simulation keeps a knight's clip and
//! time and turns them into that pair (`frames`).

use alloc::vec;
use alloc::vec::Vec;

use crate::anim::{gait, solve, Clip, Ease, Foot, Key, Prop};
use crate::math::*;
use crate::skel::{figure, Skeleton, BONES};

pub mod clip {
    pub const IDLE: u8 = 0;
    pub const WALK: u8 = 1;
    pub const RUN: u8 = 2;
    /// Overhead chop.
    pub const CHOP: u8 = 3;
    /// Sweep (a thrust for the halberd).
    pub const SWEEP: u8 = 4;
    pub const STAGGER: u8 = 5;
    /// Driven back several steps.
    pub const KNOCK: u8 = 6;
    /// Off the ground.
    pub const AIR: u8 = 7;
    /// Landing on its back.
    pub const DOWN: u8 = 8;
    pub const RISE: u8 = 9;
    /// The binding undone: it falls where it stands.
    pub const COLLAPSE: u8 = 10;
    pub const COUNT: usize = 11;
}

#[derive(Clone, Copy)]
pub struct ClipInfo {
    /// Stored frames.
    pub frames: u16,
    /// Seconds from the first frame to the last (or to the loop point).
    pub len: f32,
    pub looped: bool,
}

pub const CLIPS: [ClipInfo; clip::COUNT] = [
    ClipInfo { frames: 8, len: 2.4, looped: true },
    ClipInfo { frames: 12, len: 1.0, looped: true },
    ClipInfo { frames: 10, len: 1.0, looped: true },
    ClipInfo { frames: 17, len: 1.3, looped: false },
    ClipInfo { frames: 17, len: 1.3, looped: false },
    ClipInfo { frames: 6, len: 0.45, looped: false },
    ClipInfo { frames: 9, len: 0.8, looped: false },
    ClipInfo { frames: 6, len: 0.7, looped: true },
    ClipInfo { frames: 6, len: 0.5, looped: false },
    ClipInfo { frames: 11, len: 1.1, looped: false },
    ClipInfo { frames: 10, len: 0.9, looped: false },
];

/// Stored frames of all clips of one kind.
pub const FRAMES: usize = 112;

/// First stored frame of a clip.
pub const fn first(clip: u8) -> u16 {
    let mut at = 0;
    let mut i = 0;
    while i < clip as usize {
        at += CLIPS[i].frames;
        i += 1;
    }
    at
}

/// Walking and charging speeds in metres per second; the looped clips cover one stride per cycle.
pub const WALK_SPEED: f32 = 1.45;
pub const RUN_SPEED: f32 = 4.4;
/// Ticks into the chop and the sweep at which the weapon lands.
pub const CHOP_HIT: u32 = 39;
pub const SWEEP_HIT: u32 = 35;
/// Reach of each kind's weapon and half the angle it covers.
pub const REACH: [f32; 3] = [2.2, 3.1, 2.7];

/// The two stored frames a clip shows at `u` (seconds for a clip that ends, cycles for a loop) and the blend between them.
#[inline]
pub fn frames(clip: u8, u: f32) -> (u16, u16, f32) {
    let c = &CLIPS[clip as usize];
    let base = first(clip);
    if c.looped {
        let f = (u - floor(u)) * c.frames as f32;
        let a = (f as u16).min(c.frames - 1);
        (base + a, base + (a + 1) % c.frames, f - a as f32)
    } else {
        let f = clamp(u / c.len, 0.0, 1.0) * (c.frames - 1) as f32;
        let a = (f as u16).min(c.frames - 1);
        (base + a, base + (a + 1).min(c.frames - 1), f - a as f32)
    }
}

/// The stored frame after `f` in its clip: a loop wraps, a clip that ends stays on its last frame.
pub fn next(f: u16) -> u16 {
    let mut at = 0;
    for c in CLIPS.iter() {
        if f < at + c.frames {
            let k = f - at + 1;
            return at + if k < c.frames { k } else if c.looped { 0 } else { c.frames - 1 };
        }
        at += c.frames;
    }
    f
}

/// The clip and the time in it (seconds, or cycles for a loop) of stored frame `f`.
pub fn frame_time(f: u16) -> (u8, f32) {
    let mut at = 0;
    for (i, c) in CLIPS.iter().enumerate() {
        if f < at + c.frames {
            let k = (f - at) as f32;
            return (i as u8, if c.looped { k / c.frames as f32 } else { k / (c.frames - 1) as f32 * c.len });
        }
        at += c.frames;
    }
    (0, 0.0)
}

/// One kind of knight: its skeleton and its keyed clips.
pub struct Knight {
    pub kind: u32,
    pub skel: Skeleton,
    ready: Key,
    charge: Key,
    clips: Vec<Clip>,
}

fn prop(a: f32, r: f32, y: f32, az: f32, el: f32, two: f32, off: f32) -> Prop {
    Prop { a, r, y, az, el, roll: 0.0, two, off }
}

impl Knight {
    pub fn new(kind: u32) -> Knight {
        let skel = Skeleton::knight(kind);
        let stand = Key::stand(&skel);
        let h = skel.offset[0].y;
        let lying = -(h - 0.2);
        // How each kind carries its weapon at rest, and raised at a charge.
        let (carry, raised, off) = match kind {
            figure::KNIGHT_HALBERD => (prop(0.35, 0.36, 1.22, -0.55, 0.95, 1.0, 0.55), prop(0.6, 0.38, 1.25, 0.05, 0.25, 1.0, 0.55), 0.55),
            figure::KNIGHT_GREAT => (prop(1.05, 0.3, 1.44, 2.75, 0.5, 0.0, 0.16), prop(0.7, 0.34, 1.5, 2.6, 0.75, 1.0, 0.16), 0.16),
            _ => (prop(0.9, 0.36, 0.92, 0.25, -0.95, 0.0, 0.14), prop(0.8, 0.36, 1.3, 0.9, 0.95, 0.0, 0.14), 0.14),
        };
        // A puppet's stance: the trunk hangs a little forward, the feet apart.
        let ready = Key { bend: 0.08, prop: carry, arm_l: [0.05, 0.1, 0.22, 0.0], feet: [Foot { x: -0.15, fwd: 0.05, lift: 0.0, yaw: -0.2, pitch: 0.0 }, Foot { x: 0.16, fwd: -0.06, lift: 0.0, yaw: 0.24, pitch: 0.0 }], ..stand };
        let charge = Key { bend: 0.3, lean: 0.12, prop: raised, arm_l: [0.3, 0.2, 0.9, 0.0], ..ready };
        let two = if kind == figure::KNIGHT_SWORD { 0.0 } else { 1.0 };

        let sway = Key { side: 0.035, hip: v3(0.02, -0.006, 0.0), bend: 0.1, prop: Prop { y: carry.y - 0.012, ..carry }, ..ready };
        let sway_back = Key { side: -0.03, hip: v3(-0.015, 0.0, 0.0), bend: 0.07, twist: 0.04, ..ready };
        let idle = Clip::new(true, 2.4, vec![(0.0, ready, Ease::Smooth), (0.8, sway, Ease::Smooth), (1.7, sway_back, Ease::Smooth)]);

        // ---- the chop: the weapon goes up and back, comes over and stays down
        let step = [Foot { x: -0.14, fwd: 0.42, lift: 0.0, yaw: -0.1, pitch: 0.0 }, Foot { x: 0.18, fwd: -0.3, lift: 0.0, yaw: 0.5, pitch: -0.25 }];
        let wind = Key { twist: 0.45, lean: -0.1, bend: -0.12, hip: v3(0.0, -0.02, -0.08), prop: prop(0.7, 0.26, 1.82, 3.3, 0.85, two, off), arm_l: [1.9, 0.3, 1.2, 0.0], ..ready };
        let over = Key { twist: 0.1, bend: 0.2, hip: v3(0.0, -0.05, 0.12), prop: prop(0.25, 0.55, 1.6, 0.1, 0.9, two, off), arm_l: [1.2, 0.3, 0.8, 0.0], feet: step, ..ready };
        let land = Key { twist: -0.35, bend: 0.52, hip: v3(0.0, -0.16, 0.3), prop: prop(0.1, 0.72, 0.7, 0.0, -0.55, two, off), arm_l: [0.4, 0.3, 0.5, 0.0], feet: step, ..ready };
        let held = Key { bend: 0.48, twist: -0.3, hip: v3(0.0, -0.14, 0.28), prop: prop(0.1, 0.7, 0.62, 0.0, -0.75, two, off), ..land };
        let chop = Clip::new(false, 1.3, vec![(0.0, ready, Ease::Smooth), (0.5, wind, Ease::In), (0.6, over, Ease::Linear), (0.66, land, Ease::Out), (0.95, held, Ease::Smooth), (1.3, ready, Ease::Linear)]);

        // ---- the sweep, or the halberd's thrust
        let sweep = if kind == figure::KNIGHT_HALBERD {
            let back = Key { twist: 0.55, hip: v3(0.0, -0.04, -0.12), prop: prop(1.35, 0.22, 1.18, 0.12, 0.02, 1.0, off), ..ready };
            let out = Key { twist: -0.35, bend: 0.3, hip: v3(0.0, -0.12, 0.34), prop: prop(0.3, 0.78, 1.16, 0.0, 0.0, 1.0, off), feet: step, ..ready };
            let hold = Key { hip: v3(0.0, -0.1, 0.3), prop: prop(0.32, 0.7, 1.12, 0.0, -0.05, 1.0, off), ..out };
            Clip::new(false, 1.3, vec![(0.0, ready, Ease::Smooth), (0.48, back, Ease::In), (0.6, out, Ease::Out), (0.9, hold, Ease::Smooth), (1.3, ready, Ease::Linear)])
        } else {
            let back = Key { twist: 0.75, hip: v3(0.03, -0.05, -0.06), prop: prop(1.95, 0.36, 1.2, 2.5, 0.12, two, off), arm_l: [0.3, 0.5, 0.9, 0.0], ..ready };
            let mid = Key { twist: 0.0, bend: 0.2, hip: v3(0.0, -0.08, 0.16), prop: prop(0.5, 0.62, 1.12, 0.6, 0.02, two, off), arm_l: [0.2, 0.7, 0.6, 0.0], feet: step, ..ready };
            let out = Key { twist: -0.7, bend: 0.22, hip: v3(-0.03, -0.1, 0.24), prop: prop(-0.75, 0.58, 1.08, -1.35, -0.02, two, off), arm_l: [-0.3, 0.8, 0.5, 0.0], feet: step, ..ready };
            let hold = Key { twist: -0.6, prop: prop(-0.95, 0.5, 1.02, -1.7, -0.08, two, off), ..out };
            Clip::new(false, 1.3, vec![(0.0, ready, Ease::Smooth), (0.46, back, Ease::In), (0.56, mid, Ease::Linear), (0.63, out, Ease::Out), (0.9, hold, Ease::Smooth), (1.3, ready, Ease::Linear)])
        };

        // ---- struck
        let flung = Prop { a: carry.a + 0.5, y: carry.y - 0.08, r: carry.r + 0.1, ..carry };
        let rock = Key { lean: -0.3, bend: -0.2, twist: 0.35, hip: v3(0.0, -0.05, -0.12), prop: flung, arm_l: [-0.5, 0.6, 0.4, 0.0], feet: [Foot { x: -0.16, fwd: -0.16, lift: 0.0, yaw: -0.2, pitch: 0.0 }, Foot { x: 0.17, fwd: 0.04, lift: 0.0, yaw: 0.2, pitch: 0.0 }], ..ready };
        let stagger = Clip::new(false, 0.45, vec![(0.0, ready, Ease::Out), (0.1, rock, Ease::Smooth), (0.45, ready, Ease::Linear)]);

        let reel = Key { lean: -0.5, bend: -0.25, twist: -0.3, hip: v3(0.0, -0.1, -0.1), prop: Prop { a: carry.a + 0.9, r: carry.r + 0.2, ..flung }, arm_l: [-0.8, 0.9, 0.3, 0.0], feet: [Foot { x: -0.2, fwd: -0.32, lift: 0.08, yaw: -0.3, pitch: 0.2 }, Foot { x: 0.15, fwd: 0.1, lift: 0.0, yaw: 0.2, pitch: 0.0 }], ..ready };
        let reel2 = Key { lean: -0.32, twist: 0.2, hip: v3(0.0, -0.14, -0.05), feet: [Foot { x: -0.2, fwd: 0.06, lift: 0.0, yaw: -0.3, pitch: 0.0 }, Foot { x: 0.18, fwd: -0.3, lift: 0.07, yaw: 0.3, pitch: 0.2 }], ..reel };
        let crouch = Key { lean: 0.1, bend: 0.3, hip: v3(0.0, -0.16, 0.0), ..ready };
        let knock = Clip::new(false, 0.8, vec![(0.0, ready, Ease::Out), (0.12, reel, Ease::Smooth), (0.36, reel2, Ease::Smooth), (0.58, crouch, Ease::Smooth), (0.8, ready, Ease::Linear)]);

        // ---- off the ground, on its back, and up again
        let limp = Prop { a: 1.4, r: 0.5, y: 1.2, az: 1.6, el: 0.3, roll: 0.0, two: 0.0, off };
        let air_a = Key { lean: -1.15, bend: -0.25, ik: 0.0, legs: [[0.7, 0.3, 0.9], [0.15, 0.35, 0.4]], prop: limp, prop_w: 0.0, arm_r: [-0.5, 1.2, 0.4, 0.0], arm_l: [-0.3, 1.1, 0.5, 0.0], hip: v3(0.0, 0.1, 0.0), ..ready };
        let air_b = Key { lean: -1.5, bend: -0.1, legs: [[0.2, 0.4, 0.5], [0.6, 0.25, 0.8]], arm_r: [0.2, 1.3, 0.6, 0.0], arm_l: [-0.7, 1.0, 0.3, 0.0], ..air_a };
        let air = Clip::new(true, 0.7, vec![(0.0, air_a, Ease::Smooth), (0.35, air_b, Ease::Smooth)]);

        let flat = Key { lean: -PI * 0.5, bend: 0.0, ik: 0.0, legs: [[0.06, 0.22, 0.12], [0.1, 0.28, 0.2]], prop: limp, prop_w: 0.0, arm_r: [0.1, 0.9, 0.3, 0.0], arm_l: [0.05, 1.0, 0.25, 0.0], hip: v3(0.0, lying, 0.3), ..ready };
        let bounce = Key { hip: v3(0.0, lying + 0.14, 0.3), lean: -1.42, legs: [[0.4, 0.3, 0.5], [0.3, 0.3, 0.4]], ..flat };
        let down = Clip::new(false, 0.5, vec![(0.0, air_b, Ease::In), (0.1, flat, Ease::Out), (0.22, bounce, Ease::In), (0.36, flat, Ease::Linear), (0.5, flat, Ease::Linear)]);

        let sit = Key { lean: -0.75, bend: 0.5, hip: v3(0.0, lying + 0.12, 0.15), ik: 0.0, legs: [[1.1, 0.3, 1.3], [0.7, 0.35, 1.0]], prop: Prop { a: 1.2, r: 0.45, y: 0.5, az: 1.0, el: -0.2, roll: 0.0, two: 0.0, off }, prop_w: 0.0, arm_r: [0.3, 0.5, 0.5, 0.0], arm_l: [0.3, 0.5, 0.6, 0.0], ..ready };
        let kneel = Key { lean: 0.25, bend: 0.4, hip: v3(0.0, -0.46, 0.0), feet: [Foot { x: -0.16, fwd: 0.28, lift: 0.0, yaw: -0.1, pitch: 0.0 }, Foot { x: 0.17, fwd: -0.3, lift: 0.02, yaw: 0.2, pitch: -0.6 }], prop: Prop { a: 0.9, r: 0.42, y: 0.75, az: 0.3, el: -1.2, roll: 0.0, two: 0.0, off }, ..ready };
        let rise = Clip::new(false, 1.1, vec![(0.0, flat, Ease::Smooth), (0.34, sit, Ease::Smooth), (0.68, kneel, Ease::Smooth), (1.1, ready, Ease::Linear)]);

        // ---- undone: the knees give, the trunk folds, it ends face down
        let give = Key { hip: v3(0.0, -0.34, 0.02), bend: 0.5, lean: 0.12, prop_w: 0.0, arm_r: [0.2, 0.15, 0.2, 0.0], arm_l: [0.15, 0.12, 0.15, 0.0], feet: [Foot { x: -0.17, fwd: 0.1, lift: 0.0, yaw: -0.3, pitch: 0.0 }, Foot { x: 0.18, fwd: -0.08, lift: 0.0, yaw: 0.3, pitch: -0.3 }], ..ready };
        let fold = Key { hip: v3(0.0, -0.6, 0.12), bend: 0.75, lean: 0.75, ik: 0.4, legs: [[1.5, 0.2, 2.2], [1.3, 0.25, 2.0]], arm_r: [0.9, 0.3, 0.3, 0.0], arm_l: [1.0, 0.3, 0.3, 0.0], ..give };
        let prone = Key { hip: v3(0.0, lying - 0.02, -0.28), bend: 0.05, lean: PI * 0.5, ik: 0.0, legs: [[-0.05, 0.2, 0.25], [-0.02, 0.25, 0.5]], arm_r: [2.6, 0.5, 0.5, 0.0], arm_l: [2.3, 0.7, 0.9, 0.0], ..give };
        let collapse = Clip::new(false, 0.9, vec![(0.0, ready, Ease::In), (0.28, give, Ease::Linear), (0.52, fold, Ease::Linear), (0.74, prone, Ease::Out), (0.9, prone, Ease::Linear)]);

        Knight { kind, skel, ready, charge, clips: vec![idle, chop, sweep, stagger, knock, air, down, rise, collapse] }
    }

    /// The pose of `clip` at `u`: seconds, or cycles for a looped clip.
    pub fn key(&self, clip: u8, u: f32) -> Key {
        match clip {
            clip::IDLE => self.clips[0].sample(u * CLIPS[0].len),
            clip::WALK => {
                let mut k = Key { bend: 0.12, ..self.ready };
                let p = u * TAU;
                k.twist = 0.06 * sin(p);
                k.hip.x = 0.02 * sin(p);
                k.arm_l[0] = self.ready.arm_l[0] - 0.25 * sin(p);
                gait(&self.skel, &mut k, p, WALK_SPEED, 1.0, 2.6, 5.0);
                k
            }
            clip::RUN => {
                let mut k = self.charge;
                let p = u * TAU;
                k.twist = 0.14 * sin(p);
                k.arm_l[0] = self.charge.arm_l[0] - 0.6 * sin(p);
                k.prop.y += 0.03 * cos(p * 2.0);
                gait(&self.skel, &mut k, p, RUN_SPEED, 1.0, 2.6, 5.0);
                k
            }
            clip::AIR => self.clips[5].sample(u * CLIPS[clip::AIR as usize].len),
            c => self.clips[match c {
                clip::CHOP => 1,
                clip::SWEEP => 2,
                clip::STAGGER => 3,
                clip::KNOCK => 4,
                clip::DOWN => 6,
                clip::RISE => 7,
                _ => 8,
            }]
            .sample(u),
        }
    }

    /// Bone transforms of `clip` at `u` in the figure's frame.
    pub fn pose(&self, clip: u8, u: f32) -> [M34; BONES] {
        solve(&self.skel, &self.key(clip, u))
    }

    /// Bone transforms of stored frame `f`.
    pub fn frame(&self, f: u16) -> [M34; BONES] {
        let (clip, u) = frame_time(f);
        self.pose(clip, u)
    }
}

/// Ground a walking or charging knight covers in one cycle of its looped clip.
pub fn stride_of(kind: u32, run: bool) -> f32 {
    let skel = Skeleton::knight(kind);
    crate::anim::stride(if run { RUN_SPEED } else { WALK_SPEED }, skel.thigh() + skel.shin())
}
