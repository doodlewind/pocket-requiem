//! Poses as a few numbers, and the solver that turns them into bones.
//!
//! A `Key` does not hold a rotation per bone. It says where the feet stand,
//! how the trunk leans and twists, and where the prop in the right hand is
//! and which way it points; the solver places the legs and the arms by
//! inverse kinematics. A swing is then authored as the path of the weapon,
//! and blending two keys is a blend of scalars.
//!
//! Authored angles turn to the figure's right when positive (azimuths, yaws,
//! twists). Heights are metres above the ground under the figure.

use alloc::vec::Vec;

use crate::math::*;
use crate::skel::{bone::*, prop_rest, Skeleton, BONES};

#[derive(Clone, Copy, Debug)]
pub struct Foot {
    /// To the right of the figure's centre line.
    pub x: f32,
    /// Ahead of the figure (positive is forward).
    pub fwd: f32,
    pub lift: f32,
    /// Toe turned outward to the right.
    pub yaw: f32,
    /// Toes up.
    pub pitch: f32,
}

/// The prop in the right hand: the grip's place in cylinder coordinates about
/// the figure's axis (turned with the body's yaw), and the direction of its head.
#[derive(Clone, Copy, Debug)]
pub struct Prop {
    pub a: f32,
    pub r: f32,
    pub y: f32,
    pub az: f32,
    pub el: f32,
    /// Turn about its own axis.
    pub roll: f32,
    /// How much the left hand holds it too (0 free, 1 gripping).
    pub two: f32,
    /// Where the left hand grips, in metres toward the butt from the right hand.
    pub off: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Key {
    /// Pelvis offset from standing: right, up, forward.
    pub hip: V3,
    pub yaw: f32,
    /// Forward pitch and rightward tilt of the pelvis.
    pub lean: f32,
    pub roll: f32,
    /// The trunk over the pelvis: turn, forward bend, side bend.
    pub twist: f32,
    pub bend: f32,
    pub side: f32,
    /// Head over the chest: turn right, nod down.
    pub head: [f32; 2],
    pub prop: Prop,
    /// The free left arm: forward swing, spread, elbow, twist.
    pub arm_l: [f32; 4],
    /// The right arm when it does not follow the prop (`prop_w` 0).
    pub arm_r: [f32; 4],
    pub prop_w: f32,
    pub feet: [Foot; 2],
    /// 1: legs reach the feet; 0: legs take `legs` (thigh swing, thigh spread, knee).
    pub ik: f32,
    pub legs: [[f32; 3]; 2],
}

impl Key {
    /// Standing at ease, the prop upright at the right side.
    pub fn stand(skel: &Skeleton) -> Key {
        let hx = skel.offset[LEG_UR].x;
        let h = skel.offset[PELVIS].y;
        Key {
            hip: V3::ZERO,
            yaw: 0.0,
            lean: 0.0,
            roll: 0.0,
            twist: 0.0,
            bend: 0.0,
            side: 0.0,
            head: [0.0, 0.0],
            prop: Prop { a: 1.25, r: 0.3 * h, y: 1.08 * h, az: 0.0, el: 1.5, roll: 0.0, two: 0.0, off: 0.3 },
            arm_l: [0.06, 0.14, 0.3, 0.0],
            arm_r: [0.06, 0.14, 0.3, 0.0],
            prop_w: 1.0,
            feet: [Foot { x: -hx * 1.15, fwd: 0.0, lift: 0.0, yaw: -0.12, pitch: 0.0 }, Foot { x: hx * 1.15, fwd: 0.0, lift: 0.0, yaw: 0.12, pitch: 0.0 }],
            ik: 1.0,
            legs: [[0.0, 0.05, 0.05]; 2],
        }
    }

    pub fn blend(&self, o: &Key, t: f32) -> Key {
        let l = |a: f32, b: f32| a + (b - a) * t;
        let foot = |a: &Foot, b: &Foot| Foot { x: l(a.x, b.x), fwd: l(a.fwd, b.fwd), lift: l(a.lift, b.lift), yaw: l(a.yaw, b.yaw), pitch: l(a.pitch, b.pitch) };
        let arr4 = |a: &[f32; 4], b: &[f32; 4]| [l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2]), l(a[3], b[3])];
        let arr3 = |a: &[f32; 3], b: &[f32; 3]| [l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2])];
        Key {
            hip: self.hip.lerp(o.hip, t),
            yaw: l(self.yaw, o.yaw),
            lean: l(self.lean, o.lean),
            roll: l(self.roll, o.roll),
            twist: l(self.twist, o.twist),
            bend: l(self.bend, o.bend),
            side: l(self.side, o.side),
            head: [l(self.head[0], o.head[0]), l(self.head[1], o.head[1])],
            prop: Prop { a: l(self.prop.a, o.prop.a), r: l(self.prop.r, o.prop.r), y: l(self.prop.y, o.prop.y), az: l(self.prop.az, o.prop.az), el: l(self.prop.el, o.prop.el), roll: l(self.prop.roll, o.prop.roll), two: l(self.prop.two, o.prop.two), off: l(self.prop.off, o.prop.off) },
            arm_l: arr4(&self.arm_l, &o.arm_l),
            arm_r: arr4(&self.arm_r, &o.arm_r),
            prop_w: l(self.prop_w, o.prop_w),
            feet: [foot(&self.feet[0], &o.feet[0]), foot(&self.feet[1], &o.feet[1])],
            ik: l(self.ik, o.ik),
            legs: [arr3(&self.legs[0], &o.legs[0]), arr3(&self.legs[1], &o.legs[1])],
        }
    }
}

/// How a segment of a clip moves from its key to the next.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ease {
    Linear,
    /// Slow at both ends.
    Smooth,
    /// Accelerates into the next key: a strike.
    In,
    /// Arrives fast and settles.
    Out,
}

impl Ease {
    pub fn at(self, t: f32) -> f32 {
        match self {
            Ease::Linear => t,
            Ease::Smooth => t * t * (3.0 - 2.0 * t),
            Ease::In => t * t * t,
            Ease::Out => 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t),
        }
    }
}

/// Keys at times in seconds; a looped clip returns to its first key at `len`.
pub struct Clip {
    pub keys: Vec<(f32, Key, Ease)>,
    pub len: f32,
    pub looped: bool,
}

impl Clip {
    pub fn new(looped: bool, len: f32, keys: Vec<(f32, Key, Ease)>) -> Clip {
        Clip { keys, len, looped }
    }

    pub fn sample(&self, t: f32) -> Key {
        let n = self.keys.len();
        let t = if self.looped { t - floor(t / self.len) * self.len } else { clamp(t, 0.0, self.len) };
        let mut i = 0;
        while i + 1 < n && self.keys[i + 1].0 <= t {
            i += 1;
        }
        let (t0, a, ease) = &self.keys[i];
        let (t1, b) = if i + 1 < n {
            (self.keys[i + 1].0, &self.keys[i + 1].1)
        } else if self.looped {
            (self.len, &self.keys[0].1)
        } else {
            return *a;
        };
        let u = saturate((t - t0) / (t1 - t0).max(1e-5));
        a.blend(b, ease.at(u))
    }
}

/// Unit vector for an authored azimuth (right is positive) and elevation.
#[inline]
pub fn dir_of(az: f32, el: f32) -> V3 {
    let c = cos(el);
    v3(sin(az) * c, sin(el), -cos(az) * c)
}

/// Two-bone reach from `from` to `target`: the first bone's direction and the
/// clamped distance. `hint` is where the joint in the middle points.
fn reach(from: V3, target: V3, l1: f32, l2: f32, hint: V3) -> (V3, V3, V3) {
    let v = target - from;
    let d = clamp(v.len(), abs(l1 - l2) + 0.02, (l1 + l2) * 0.999);
    let dir = v.norm_or(v3(0.0, -1.0, 0.0));
    let h = (hint - dir * hint.dot(dir)).norm_or(v3(0.0, 0.0, 1.0));
    let a = acos((l1 * l1 + d * d - l2 * l2) / (2.0 * l1 * d));
    let first = dir * cos(a) + h * sin(a);
    let mid = from + first * l1;
    let second = (from + dir * d - mid).norm_or(dir);
    (first, second, h)
}

/// A bone lying along `along` (its -Y) whose +Z is as close to `back` as that allows.
fn limb(along: V3, back: V3) -> M3 {
    let y = -along;
    let x = y.cross(back).norm_or(v3(1.0, 0.0, 0.0));
    M3 { x, y, z: x.cross(y) }
}

/// Bone transforms of `key` in the figure's frame: origin on the ground under it, facing -Z.
pub fn solve(skel: &Skeleton, key: &Key) -> [M34; BONES] {
    let mut w = [M34::ID; BONES];
    let o = &skel.offset;
    let yaw = M3::rot_y(-key.yaw);
    let pelvis_r = yaw.mul(&M3::rot_x(-key.lean)).mul(&M3::rot_z(-key.roll));
    let pelvis_t = v3(key.hip.x, o[PELVIS].y + key.hip.y, -key.hip.z);
    w[PELVIS] = M34::new(pelvis_r, pelvis_t);

    // The trunk: the turn and the bends are shared between the spine and the chest.
    let part = |k: f32| M3::rot_y(-key.twist * k).mul(&M3::rot_x(-key.bend * k)).mul(&M3::rot_z(-key.side * k));
    w[SPINE] = M34::new(pelvis_r.mul(&part(0.4)), w[PELVIS].apply(o[SPINE]));
    w[CHEST] = M34::new(w[SPINE].r.mul(&part(0.6)), w[SPINE].apply(o[CHEST]));
    w[NECK] = M34::new(w[CHEST].r.mul(&M3::rot_y(-key.head[0] * 0.4).mul(&M3::rot_x(-key.head[1] * 0.4))), w[CHEST].apply(o[NECK]));
    w[HEAD] = M34::new(w[NECK].r.mul(&M3::rot_y(-key.head[0] * 0.6).mul(&M3::rot_x(-key.head[1] * 0.6))), w[NECK].apply(o[HEAD]));
    for c in [CLAV_L, CLAV_R] {
        w[c] = M34::new(w[CHEST].r, w[CHEST].apply(o[c]));
        w[c + 1].t = w[c].apply(o[c + 1]);
    }

    // ---- legs
    let (l1, l2) = (skel.thigh(), skel.shin());
    let ankle = skel.ankle();
    for (s, side) in [(0usize, -1.0f32), (1, 1.0)] {
        let (u, l, f, k) = if s == 0 { (LEG_UL, LEG_LL, FOOT_L, SKIRT_L) } else { (LEG_UR, LEG_LR, FOOT_R, SKIRT_R) };
        let from = w[PELVIS].apply(o[u]);
        let foot = &key.feet[s];
        // Reaching the foot.
        let target = v3(foot.x, ankle + foot.lift, -foot.fwd);
        let knee_yaw = key.yaw * 0.5 + foot.yaw;
        // The knee points ahead: the bones' backs face away from it.
        let (first, second, h) = reach(from, target, l1, l2, v3(sin(knee_yaw), 0.0, -cos(knee_yaw)));
        let mut thigh = limb(first, -h);
        let mut shin = limb(second, -h);
        let mut sole = M3::rot_y(-(foot.yaw + key.yaw * 0.5)).mul(&M3::rot_x(foot.pitch));
        if key.ik < 0.999 {
            // The legs as joint angles, hanging from the pelvis.
            let a = &key.legs[s];
            let ft = pelvis_r.mul(&Quat::euler(a[0], 0.0, side * a[1]).m3());
            let fs = ft.mul(&M3::rot_x(-a[2]));
            let k = key.ik;
            thigh = ft.quat().nlerp(thigh.quat(), k).m3();
            shin = fs.quat().nlerp(shin.quat(), k).m3();
            sole = fs.mul(&M3::rot_x(0.2)).quat().nlerp(sole.quat(), k).m3();
        }
        w[u] = M34::new(thigh, from);
        w[l] = M34::new(shin, w[u].apply(o[l]));
        w[f] = M34::new(sole, w[l].apply(o[f]));
        // The skirt follows the thigh part of the way.
        w[k] = M34::new(pelvis_r.quat().nlerp(thigh.quat(), 0.55).m3(), from);
    }

    // ---- the prop and the right arm
    let (a1, a2) = (skel.upper(), skel.fore());
    let p = &key.prop;
    let grip = yaw.apply(v3(sin(p.a) * p.r, 0.0, -cos(p.a) * p.r)) + v3(key.hip.x, p.y + key.hip.y, -key.hip.z);
    let dir = yaw.apply(dir_of(p.az, p.el));
    let hand_for = |shoulder: V3, at: V3, flip: f32| {
        // The hand's -Z lies along the prop; its -Y continues the forearm as far as that allows.
        let fd = (at - shoulder).norm_or(v3(0.0, -1.0, 0.0));
        let y = -(fd - dir * fd.dot(dir)).norm_or(yaw.apply(v3(flip, 0.0, 0.0)));
        let z = -dir;
        M3 { x: y.cross(z), y, z }
    };
    let chest_r = w[CHEST].r;
    let free = |s: f32, a: &[f32; 4], shoulder: V3| {
        let upper = chest_r.mul(&Quat::euler(a[0], -s * a[3], s * a[1]).m3());
        let fore = upper.mul(&M3::rot_x(a[2]));
        let elbow = shoulder + upper.apply(v3(0.0, -a1, 0.0));
        (upper, fore, elbow, elbow + fore.apply(v3(0.0, -a2, 0.0)))
    };
    let arm = |shoulder: V3, wrist: V3, hand: M3, hint: V3| {
        // The elbow points where the hint says: the bones' backs face it.
        let (first, second, h) = reach(shoulder, wrist, a1, a2, hint);
        let upper = limb(first, h);
        let fore = limb(second, h);
        let elbow = shoulder + first * a1;
        (upper, fore, elbow, elbow + second * a2, hand)
    };
    {
        let shoulder = w[ARM_UR].t;
        let roll = M3::rot_z(p.roll);
        let held = hand_for(shoulder, grip, 1.0).mul(&roll);
        let (upper, fore, elbow, wrist, hand) = if key.prop_w > 0.001 {
            let wrist = grip - held.apply(skel.palm);
            let hint = yaw.apply(v3(0.55, -0.75, 0.4));
            let got = arm(shoulder, wrist, held, hint);
            if key.prop_w < 0.999 {
                let f = free(1.0, &key.arm_r, shoulder);
                let k = key.prop_w;
                let up = f.0.quat().nlerp(got.0.quat(), k).m3();
                let fo = f.1.quat().nlerp(got.1.quat(), k).m3();
                let e = shoulder + up.apply(v3(0.0, -a1, 0.0));
                (up, fo, e, e + fo.apply(v3(0.0, -a2, 0.0)), f.1.quat().nlerp(held.quat(), k).m3())
            } else {
                got
            }
        } else {
            let f = free(1.0, &key.arm_r, shoulder);
            (f.0, f.1, f.2, f.3, f.1)
        };
        w[ARM_UR] = M34::new(upper, shoulder);
        w[ARM_LR] = M34::new(fore, elbow);
        w[HAND_R] = M34::new(hand, wrist);
        w[PROP] = M34::new(hand.mul(&prop_rest().m3()), w[HAND_R].apply(skel.palm));
    }
    // ---- the left arm: free, or on the prop below the right hand
    {
        let shoulder = w[ARM_UL].t;
        let f = free(-1.0, &key.arm_l, shoulder);
        let (upper, fore, elbow, wrist, hand) = if p.two > 0.001 {
            let prop_dir = w[PROP].r.y;
            let at = w[PROP].t - prop_dir * p.off;
            let fd = (at - shoulder).norm_or(v3(0.0, -1.0, 0.0));
            let y = -(fd - prop_dir * fd.dot(prop_dir)).norm_or(yaw.apply(v3(-1.0, 0.0, 0.0)));
            let z = -prop_dir;
            let held = M3 { x: y.cross(z), y, z };
            let target = at - held.apply(skel.palm);
            let wrist = f.3.lerp(target, p.two);
            let hint = yaw.apply(v3(-0.55, -0.75, 0.4));
            let got = arm(shoulder, wrist, f.1.quat().nlerp(held.quat(), p.two).m3(), hint);
            (got.0, got.1, got.2, got.3, got.4)
        } else {
            (f.0, f.1, f.2, f.3, f.1)
        };
        w[ARM_UL] = M34::new(upper, shoulder);
        w[ARM_LL] = M34::new(fore, elbow);
        w[HAND_L] = M34::new(hand, wrist);
    }

    // ---- hair tails hang straight down from the head until something moves them
    for t in [TAIL_L0, TAIL_R0] {
        w[t] = M34::new(yaw, w[HEAD].apply(o[t]));
        w[t + 1] = M34::new(yaw, w[t].apply(o[t + 1]));
        w[t + 2] = M34::new(yaw, w[t + 1].apply(o[t + 2]));
    }
    w
}

/// Places a figure-frame pose in the world: turned to `yaw` (heading convention) at `pos`.
pub fn place(w: &mut [M34; BONES], pos: V3, yaw: f32) {
    let root = M34::new(M3::rot_y(yaw), pos);
    for m in w.iter_mut() {
        *m = root.mul(m);
    }
}

/// Skin matrices: world × bind⁻¹.
pub fn skin(world: &[M34; BONES], bind_inv: &[M34; BONES]) -> [M34; BONES] {
    let mut s = [M34::ID; BONES];
    for i in 0..BONES {
        s[i] = world[i].mul(&bind_inv[i]);
    }
    s
}

/// Ground covered by one full stride (two steps) at `speed`.
pub fn stride(speed: f32, leg: f32) -> f32 {
    clamp(0.9 + 0.32 * speed, 0.9, 4.2) * leg / 0.82
}

/// Walking and running by foot placement: each foot alternates a stance on
/// the ground, moving back under the body at the body's speed, and a swing
/// forward through the air. `phase` advances by distance over `stride`.
/// Writes the feet and the pelvis's height into `key`.
pub fn gait(skel: &Skeleton, key: &mut Key, phase: f32, speed: f32, moving: f32, run_from: f32, run_to: f32) {
    let run = smoothstep(run_from, run_to, speed) * moving;
    let duty = lerp(0.62, 0.36, run);
    let leg = skel.thigh() + skel.shin();
    let hx = skel.offset[LEG_UR].x;
    let half = min(duty * stride(speed, leg) * 0.5, leg * 0.56) * moving;
    // Walking, the hips are lowest as the heel strikes; running, in the middle of the stance.
    let bob = lerp(0.02, 0.0, run) * cos(phase * 2.0) + run * 0.05 * cos((phase - PI * duty) * 2.0);
    key.hip.y += -leg * lerp(0.006, 0.05, run) * moving - bob * moving;
    // Steps are as high as the legs are long.
    let size = leg / 0.82;
    for (s, side, offset) in [(0usize, -1.0f32, 0.0f32), (1, 1.0, 0.5)] {
        let u = (phase / TAU + offset) - floor(phase / TAU + offset);
        let (z, lift, pitch) = if u < duty {
            let k = u / duty;
            // Heel first, flat through the middle, then the heel peels off.
            let toe = if k < 0.15 { 0.3 * (1.0 - k / 0.15) } else if k > 0.68 { -0.8 * (k - 0.68) / 0.32 } else { 0.0 };
            (half - 2.0 * half * k, max(k - 0.72, 0.0) / 0.28 * 0.06, toe)
        } else {
            let k = (u - duty) / (1.0 - duty);
            let e = k * k * (3.0 - 2.0 * k);
            // The foot swings through; at a run the heel kicks up behind first.
            (-half + 2.0 * half * e, (0.06 * (1.0 - k) + lerp(0.07, 0.14, run) * sin(PI * k) + run * 0.14 * sin(PI * min(k * 1.6, 1.0))) * size, lerp(-0.8, 0.3, smoothstep(0.25, 0.95, k)))
        };
        let rest = key.feet[s];
        key.feet[s] = Foot { x: lerp(rest.x, side * lerp(hx, hx * 0.6, run), moving), fwd: lerp(rest.fwd, z, moving), lift: rest.lift + lift * moving, yaw: rest.yaw * (1.0 - 0.6 * moving), pitch: rest.pitch + pitch * moving };
    }
}
