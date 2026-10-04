//! Skeletons. Every figure (the mage, the knights, the demon) shares one
//! bone list: a nineteen-bone body, two skirt bones that follow the thighs,
//! two three-bone hair tails and a prop bone in the right hand.
//!
//! Renderers skin with `skin[i] = world[i] × bind[i]⁻¹`; models are built in
//! the bind pose (`Skeleton::bind`: arms out, legs slightly apart, tails
//! standing out behind the shoulders, the prop along the hand's forward axis).
//!
//! Local frame: +Y up, -Z forward, +X right. A limb points down its -Y axis.

use crate::math::*;

pub const BONES: usize = 28;

pub mod bone {
    pub const PELVIS: usize = 0;
    pub const SPINE: usize = 1;
    pub const CHEST: usize = 2;
    pub const NECK: usize = 3;
    pub const HEAD: usize = 4;
    pub const CLAV_L: usize = 5;
    pub const ARM_UL: usize = 6;
    pub const ARM_LL: usize = 7;
    pub const HAND_L: usize = 8;
    pub const CLAV_R: usize = 9;
    pub const ARM_UR: usize = 10;
    pub const ARM_LR: usize = 11;
    pub const HAND_R: usize = 12;
    pub const LEG_UL: usize = 13;
    pub const LEG_LL: usize = 14;
    pub const FOOT_L: usize = 15;
    pub const LEG_UR: usize = 16;
    pub const LEG_LR: usize = 17;
    pub const FOOT_R: usize = 18;
    pub const SKIRT_L: usize = 19;
    pub const SKIRT_R: usize = 20;
    pub const TAIL_L0: usize = 21;
    pub const TAIL_L1: usize = 22;
    pub const TAIL_L2: usize = 23;
    pub const TAIL_R0: usize = 24;
    pub const TAIL_R1: usize = 25;
    pub const TAIL_R2: usize = 26;
    pub const PROP: usize = 27;
}
use bone::*;

pub const PARENT: [usize; BONES] = [0, 0, 1, 2, 3, 2, 5, 6, 7, 2, 9, 10, 11, 0, 13, 14, 0, 16, 17, 0, 0, 4, 21, 22, 4, 24, 25, 12];

pub type Rot = [Quat; BONES];

/// The prop's rest rotation in the hand: its +Y (toward the staff's head, a blade's point) lies along the hand's -Z.
pub fn prop_rest() -> Quat {
    Quat::euler(-PI * 0.5, 0.0, 0.0)
}

/// Figure kinds: who a skeleton, a bind pose or a model belongs to.
pub mod figure {
    pub const MAGE: u32 = 0;
    /// Longsword.
    pub const KNIGHT_SWORD: u32 = 1;
    /// Halberd, two hands.
    pub const KNIGHT_HALBERD: u32 = 2;
    /// Greatsword carried on the shoulder, heavy plate.
    pub const KNIGHT_GREAT: u32 = 3;
    pub const DEMON: u32 = 4;
    pub const COUNT: u32 = 5;
}

/// Joint positions in the parent's frame; the pelvis entry is its height above the feet.
#[derive(Clone, Copy)]
pub struct Skeleton {
    pub offset: [V3; BONES],
    /// Length of one hair-tail segment.
    pub tail: f32,
    /// The palm, in the hand's frame: where a prop's grip sits.
    pub palm: V3,
}

pub struct Build {
    pub pelvis: f32,
    pub spine: f32,
    pub chest: f32,
    pub neck: f32,
    pub head: f32,
    pub clav: (f32, f32),
    pub shoulder: f32,
    pub upper: f32,
    pub fore: f32,
    pub hip: (f32, f32),
    pub thigh: f32,
    pub shin: f32,
    /// Where a hair tail leaves the head (x is mirrored), and the length of a segment.
    pub tail_at: V3,
    pub tail: f32,
}

impl Skeleton {
    pub fn build(b: &Build) -> Skeleton {
        let mut o = [V3::ZERO; BONES];
        o[PELVIS] = v3(0.0, b.pelvis, 0.0);
        o[SPINE] = v3(0.0, b.spine, 0.0);
        o[CHEST] = v3(0.0, b.chest, 0.0);
        o[NECK] = v3(0.0, b.neck, 0.0);
        o[HEAD] = v3(0.0, b.head, 0.0);
        for (s, c, u, l, h) in [(-1.0, CLAV_L, ARM_UL, ARM_LL, HAND_L), (1.0, CLAV_R, ARM_UR, ARM_LR, HAND_R)] {
            o[c] = v3(s * b.clav.0, b.clav.1, 0.0);
            o[u] = v3(s * b.shoulder, 0.0, 0.0);
            o[l] = v3(0.0, -b.upper, 0.0);
            o[h] = v3(0.0, -b.fore, 0.0);
        }
        for (s, u, l, f, k) in [(-1.0, LEG_UL, LEG_LL, FOOT_L, SKIRT_L), (1.0, LEG_UR, LEG_LR, FOOT_R, SKIRT_R)] {
            o[u] = v3(s * b.hip.0, b.hip.1, 0.0);
            o[l] = v3(0.0, -b.thigh, 0.0);
            o[f] = v3(0.0, -b.shin, 0.0);
            o[k] = o[u];
        }
        for (s, t) in [(-1.0, TAIL_L0), (1.0, TAIL_R0)] {
            o[t] = v3(s * b.tail_at.x, b.tail_at.y, b.tail_at.z);
            o[t + 1] = v3(0.0, -b.tail, 0.0);
            o[t + 2] = v3(0.0, -b.tail, 0.0);
        }
        let palm = v3(0.0, -0.062 * b.fore / 0.25, -0.012);
        o[PROP] = palm;
        Skeleton { offset: o, tail: b.tail, palm }
    }

    /// The mage: 1.52 m, slight, with a head a little large for the body.
    pub fn mage() -> Skeleton {
        Skeleton::build(&Build { pelvis: 0.82, spine: 0.085, chest: 0.135, neck: 0.215, head: 0.055, clav: (0.03, 0.165), shoulder: 0.118, upper: 0.225, fore: 0.21, hip: (0.078, -0.05), thigh: 0.365, shin: 0.345, tail_at: v3(0.118, 0.172, 0.052), tail: 0.22 })
    }

    /// A knight in plate: 1.84 m to the collar (there is no head), broad in the shoulder.
    pub fn knight(kind: u32) -> Skeleton {
        let heavy = kind == figure::KNIGHT_GREAT;
        Skeleton::build(&Build {
            pelvis: 0.98,
            spine: 0.11,
            chest: 0.17,
            neck: 0.28,
            head: 0.06,
            clav: (0.05, if heavy { 0.225 } else { 0.215 }),
            shoulder: if heavy { 0.185 } else { 0.165 },
            upper: 0.295,
            fore: 0.265,
            hip: (0.105, -0.065),
            thigh: 0.44,
            shin: 0.415,
            tail_at: v3(0.1, 0.1, 0.05),
            tail: 0.2,
        })
    }

    /// The demon: 1.6 m, slender; her tails are two long braids.
    pub fn demon() -> Skeleton {
        Skeleton::build(&Build { pelvis: 0.875, spine: 0.09, chest: 0.14, neck: 0.225, head: 0.055, clav: (0.03, 0.17), shoulder: 0.12, upper: 0.235, fore: 0.22, hip: (0.082, -0.05), thigh: 0.39, shin: 0.375, tail_at: v3(0.1, 0.05, -0.078), tail: 0.2 })
    }

    pub fn of(kind: u32) -> Skeleton {
        match kind {
            figure::MAGE => Skeleton::mage(),
            figure::DEMON => Skeleton::demon(),
            k => Skeleton::knight(k),
        }
    }

    /// Height of the ankle joint above the ground when standing straight.
    pub fn ankle(&self) -> f32 {
        self.offset[PELVIS].y + self.offset[LEG_UR].y + self.offset[LEG_LR].y + self.offset[FOOT_R].y
    }
    pub fn thigh(&self) -> f32 {
        -self.offset[LEG_LR].y
    }
    pub fn shin(&self) -> f32 {
        -self.offset[FOOT_R].y
    }
    pub fn upper(&self) -> f32 {
        -self.offset[ARM_LR].y
    }
    pub fn fore(&self) -> f32 {
        -self.offset[HAND_R].y
    }

    /// World transforms from local rotations. `pelvis` places the pelvis joint.
    pub fn fk(&self, pelvis: &M34, rot: &Rot) -> [M34; BONES] {
        let mut w = [M34::ID; BONES];
        w[0] = M34::new(pelvis.r.mul(&rot[0].m3()), pelvis.t);
        for i in 1..BONES {
            let p = w[PARENT[i]];
            w[i] = M34::new(p.r.mul(&rot[i].m3()), p.apply(self.offset[i]));
        }
        w
    }

    /// The pose models are built in: standing at the origin, facing -Z, arms 37° out, legs slightly apart.
    pub fn bind(&self) -> [M34; BONES] {
        let mut r = [Quat::ID; BONES];
        r[ARM_UL] = Quat::euler(0.0, 0.0, -0.65);
        r[ARM_UR] = Quat::euler(0.0, 0.0, 0.65);
        r[ARM_LL] = Quat::euler(0.12, 0.0, 0.0);
        r[ARM_LR] = Quat::euler(0.12, 0.0, 0.0);
        r[LEG_UL] = Quat::euler(0.0, 0.0, -0.1);
        r[LEG_UR] = Quat::euler(0.0, 0.0, 0.1);
        r[PROP] = prop_rest();
        // The hair tails stand out behind the shoulders, clear of the body, so a model's hair and trunk stay separate surfaces.
        r[TAIL_L0] = Quat::euler(-0.6, 0.0, -0.3);
        r[TAIL_R0] = Quat::euler(-0.6, 0.0, 0.3);
        self.fk(&M34::new(M3::ID, self.offset[PELVIS]), &r)
    }

    pub fn bind_inverse(&self) -> [M34; BONES] {
        self.bind().map(|m| m.inverse_rigid())
    }
}
