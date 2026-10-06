//! The mage's motion: a clip per move, the walk and the hover, and the parts
//! that swing after her (two hair tails as chains of particles, the skirt).
//!
//! The staff leads. A key says where the staff is and which way its head
//! points; her arms follow it (`anim::solve`). The clip of a move is indexed
//! by the move's id in `moves.rs`, and its keys sit at that move's ticks.

use alloc::vec;
use alloc::vec::Vec;

use crate::anim::{gait, place, skin, solve, Clip, Ease, Foot, Key, Prop};
use crate::math::*;
use crate::moves::mv;
use crate::skel::{bone::*, Skeleton, BONES};

/// The staff along the prop bone's +Y: the orb in its crescent, and the butt.
pub const STAFF_HEAD: f32 = 0.74;
pub const STAFF_BUTT: f32 = -0.74;

/// What the animator is asked to show.
pub mod show {
    pub const FREE: u8 = 0;
    pub const MOVE: u8 = 1;
    pub const GUARD: u8 = 2;
    pub const HIT: u8 = 3;
    pub const DOWN: u8 = 4;
}

pub struct AnimIn {
    pub pos: V3,
    /// Facing, with the spin of a whirling strike added.
    pub yaw: f32,
    pub speed: f32,
    pub show: u8,
    pub mv: u8,
    /// Ticks into the move, the hit or the fall.
    pub t: f32,
    pub phase: f32,
    /// 0 on foot, 1 hovering.
    pub hover: f32,
    pub tick: u32,
    /// A strike is holding the frame: nothing advances.
    pub frozen: bool,
}

const TAIL_POINTS: usize = 4;

pub struct Animator {
    pub skel: Skeleton,
    bind_inv: [M34; BONES],
    clips: Vec<Clip>,
    idle: Clip,
    stance: Key,
    run: Key,
    hover: Key,
    guard: Key,
    hit: Clip,
    down: Clip,
    from: Key,
    cur: Key,
    mix: f32,
    mix_len: f32,
    tag: u32,
    tails: [[V3; TAIL_POINTS]; 2],
    tails_prev: [[V3; TAIL_POINTS]; 2],
    live: bool,
    last_yaw: f32,
    pub world: [M34; BONES],
    pub skin: [M34; BONES],
    /// The orb at the staff's head, and its butt, in the world.
    pub head: V3,
    pub butt: V3,
    /// Unit vector along the staff toward its head.
    pub staff: V3,
}

fn prop(a: f32, r: f32, y: f32, az: f32, el: f32, two: f32) -> Prop {
    Prop { a, r, y, az, el, roll: 0.0, two, off: 0.32, slide: 0.0 }
}
fn foot(x: f32, fwd: f32, lift: f32, yaw: f32, pitch: f32) -> Foot {
    Foot { x, fwd, lift, yaw, pitch }
}
/// Seconds of a tick count.
fn at(tick: u32) -> f32 {
    tick as f32 / 60.0
}

impl Animator {
    pub fn new() -> Animator {
        let skel = Skeleton::mage();
        let stand = Key::stand(&skel);
        let lying = -(skel.offset[PELVIS].y - 0.15);

        // At ease: the staff upright ahead of her right side, its butt a hand above the ground. She holds it
        // above its middle, at the height where her forearm lies level and the upper arm hangs.
        let ease = Key { prop: Prop { slide: 0.13, ..prop(0.96, 0.36, 1.0, 0.0, 1.5, 0.0) }, arm_l: [0.04, 0.09, 0.18, 0.0], feet: [foot(-0.075, 0.02, 0.0, -0.1, 0.0), foot(0.085, -0.02, 0.0, 0.16, 0.0)], ..stand };
        let breathe = Key { bend: 0.025, hip: v3(0.004, -0.004, 0.0), head: [0.03, 0.02], prop: Prop { slide: 0.13, ..prop(0.96, 0.36, 0.996, 0.0, 1.49, 0.0) }, ..ease };
        let idle = Clip::new(true, 3.6, vec![(0.0, ease, Ease::Smooth), (1.8, breathe, Ease::Smooth)]);

        // Between strikes: side on, the staff across her in both hands.
        let wide = [foot(-0.11, 0.17, 0.0, -0.08, 0.0), foot(0.13, -0.15, 0.0, 0.45, 0.0)];
        let stance = Key { twist: 0.22, hip: v3(0.0, -0.045, 0.0), bend: 0.06, prop: prop(0.55, 0.3, 0.98, -0.6, 0.75, 1.0), feet: wide, ..stand };
        // Running, she carries it at its middle beside her hip, the arm let down, the head leading.
        let run = Key { bend: 0.13, lean: 0.07, head: [0.0, -0.1], prop: prop(1.24, 0.26, 0.775, -0.04, 0.38, 0.0), arm_l: [0.1, 0.12, 0.9, 0.0], ..ease };
        // Hovering, the same carry along her side, the staff level.
        let hover = Key { lean: 0.62, bend: -0.12, head: [0.0, -0.4], hip: v3(0.0, 0.34, 0.0), ik: 0.0, legs: [[-0.25, 0.03, 0.3], [-0.42, 0.03, 0.6]], prop: prop(1.65, 0.24, 0.78, 0.0, 0.05, 0.0), arm_l: [-0.5, 0.3, 0.25, 0.0], ..ease };
        // The barrier: the staff level across her chest, a hand before each shoulder.
        let guard = Key { hip: v3(0.0, -0.09, -0.02), bend: 0.1, prop: prop(0.44, 0.4, 1.12, 1.45, 0.1, 1.0), feet: wide, ..stand };
        let flinch = Key { lean: -0.24, bend: -0.14, twist: 0.3, hip: v3(0.0, -0.06, -0.1), prop: prop(1.0, 0.3, 1.0, 0.6, 1.1, 0.0), arm_l: [-0.3, 0.5, 0.6, 0.0], ..stance };
        let hit = Clip::new(false, at(22), vec![(0.0, stance, Ease::Out), (at(4), flinch, Ease::Smooth), (at(22), stance, Ease::Linear)]);
        let flat = Key { lean: -PI * 0.5, ik: 0.0, legs: [[0.06, 0.16, 0.12], [0.1, 0.2, 0.2]], prop_w: 0.0, arm_r: [0.1, 0.8, 0.3, 0.0], arm_l: [0.05, 0.9, 0.25, 0.0], hip: v3(0.0, lying, 0.25), ..stand };
        let down = Clip::new(false, at(40), vec![(0.0, flinch, Ease::In), (at(16), flat, Ease::Linear), (at(40), flat, Ease::Linear)]);

        let mut clips: Vec<Clip> = Vec::with_capacity(mv::COUNT);
        clips.push(Clip::new(false, 1.0, vec![(0.0, stance, Ease::Linear)]));

        // ---- L1: right to left
        let step_l = [foot(-0.1, 0.34, 0.0, -0.05, 0.0), foot(0.14, -0.2, 0.0, 0.5, -0.2)];
        let step_r = [foot(-0.14, -0.2, 0.0, -0.5, -0.2), foot(0.1, 0.34, 0.0, 0.05, 0.0)];
        let w1 = Key { twist: 0.82, hip: v3(0.02, -0.07, -0.05), prop: prop(1.9, 0.3, 1.03, 2.6, 0.14, 1.0), ..stance };
        let m1 = Key { twist: 0.0, bend: 0.14, hip: v3(0.0, -0.1, 0.1), prop: prop(0.35, 0.42, 1.0, 0.3, 0.02, 1.0), feet: step_l, ..stance };
        let t1 = Key { twist: -0.82, bend: 0.12, hip: v3(-0.02, -0.1, 0.16), prop: prop(-1.1, 0.4, 0.98, -1.6, 0.0, 1.0), feet: step_l, ..stance };
        let s1 = Key { twist: -0.66, hip: v3(-0.02, -0.08, 0.14), prop: prop(-1.3, 0.34, 0.97, -2.1, 0.1, 1.0), feet: step_l, ..stance };
        clips.push(Clip::new(false, at(30), vec![(0.0, stance, Ease::Out), (at(6), w1, Ease::In), (at(10), m1, Ease::Linear), (at(13), t1, Ease::Out), (at(20), s1, Ease::Smooth), (at(30), stance, Ease::Linear)]));

        // ---- L2: back from the left
        let w2 = Key { twist: -0.8, hip: v3(-0.02, -0.08, 0.1), prop: prop(-1.5, 0.32, 1.0, -2.45, 0.12, 1.0), feet: step_l, ..stance };
        let m2 = Key { twist: 0.0, bend: 0.14, hip: v3(0.0, -0.1, 0.16), prop: prop(0.0, 0.44, 1.02, -0.2, 0.02, 1.0), feet: step_l, ..stance };
        let t2 = Key { twist: 0.84, bend: 0.12, hip: v3(0.02, -0.1, 0.2), prop: prop(1.3, 0.4, 1.0, 1.7, 0.0, 1.0), feet: step_r, ..stance };
        let s2 = Key { twist: 0.62, hip: v3(0.02, -0.08, 0.16), prop: prop(1.5, 0.34, 0.99, 2.1, 0.12, 1.0), feet: step_r, ..stance };
        clips.push(Clip::new(false, at(30), vec![(0.0, w2, Ease::Smooth), (at(5), w2, Ease::In), (at(9), m2, Ease::Linear), (at(12), t2, Ease::Out), (at(19), s2, Ease::Smooth), (at(30), stance, Ease::Linear)]));

        // ---- L3: overhead, down onto the ground
        let up3 = Key { bend: -0.16, lean: -0.08, twist: 0.15, hip: v3(0.0, 0.03, -0.04), head: [0.0, -0.15], prop: prop(0.25, 0.2, 1.5, 3.2, 0.75, 1.0), feet: [foot(-0.1, 0.1, 0.0, -0.08, 0.0), foot(0.13, -0.12, 0.03, 0.4, -0.3)], ..stance };
        let dn3 = Key { bend: 0.56, twist: -0.1, hip: v3(0.0, -0.2, 0.2), head: [0.0, -0.3], prop: prop(0.05, 0.5, 0.72, 0.0, -0.32, 1.0), feet: [foot(-0.11, 0.42, 0.0, -0.05, 0.0), foot(0.14, -0.26, 0.0, 0.5, -0.35)], ..stance };
        let hold3 = Key { bend: 0.5, hip: v3(0.0, -0.18, 0.2), prop: prop(0.05, 0.48, 0.7, 0.0, -0.38, 1.0), ..dn3 };
        clips.push(Clip::new(false, at(36), vec![(0.0, stance, Ease::Out), (at(8), up3, Ease::In), (at(13), dn3, Ease::Out), (at(21), hold3, Ease::Smooth), (at(36), stance, Ease::Linear)]));

        // ---- L4: a full turn with the staff held out (the turn itself comes from the move's spin)
        let coil = Key { twist: -0.5, hip: v3(0.0, -0.11, 0.0), prop: prop(-0.6, 0.3, 0.95, -1.4, 0.05, 1.0), ..stance };
        let out4 = Key { twist: 0.32, bend: 0.05, hip: v3(0.0, -0.02, 0.0), prop: prop(1.2, 0.5, 1.05, 1.35, 0.0, 1.0), feet: [foot(-0.07, 0.04, 0.07, -0.1, -0.4), foot(0.09, -0.04, 0.05, 0.2, -0.4)], ..stance };
        let s4 = Key { twist: 0.52, hip: v3(0.0, -0.09, 0.0), prop: prop(1.4, 0.36, 1.0, 1.9, 0.1, 1.0), ..stance };
        clips.push(Clip::new(false, at(40), vec![(0.0, stance, Ease::Out), (at(5), coil, Ease::In), (at(8), out4, Ease::Linear), (at(22), out4, Ease::Out), (at(28), s4, Ease::Smooth), (at(40), stance, Ease::Linear)]));

        // ---- L5: drawn back, then driven forward
        let lunge = [foot(-0.11, 0.46, 0.0, -0.05, 0.0), foot(0.15, -0.36, 0.0, 0.55, -0.4)];
        let back5 = Key { twist: 0.72, hip: v3(0.02, -0.1, -0.1), prop: prop(1.5, 0.2, 1.08, 0.08, 0.04, 1.0), ..stance };
        let out5 = Key { twist: -0.42, bend: 0.26, hip: v3(0.0, -0.15, 0.3), head: [0.0, -0.2], prop: prop(0.12, 0.5, 1.1, 0.0, 0.03, 1.0), feet: lunge, ..stance };
        let hold5 = Key { hip: v3(0.0, -0.13, 0.27), prop: prop(0.14, 0.46, 1.09, 0.0, 0.05, 1.0), ..out5 };
        clips.push(Clip::new(false, at(54), vec![(0.0, stance, Ease::Out), (at(10), back5, Ease::In), (at(18), out5, Ease::Out), (at(32), hold5, Ease::Smooth), (at(54), stance, Ease::Linear)]));

        // ---- the beam: the staff levelled, a kick as it fires
        let brace = [foot(-0.12, 0.26, 0.0, -0.05, 0.0), foot(0.14, -0.26, 0.0, 0.55, 0.0)];
        let raise = Key { twist: 0.3, prop: prop(0.9, 0.3, 1.25, 0.5, 0.9, 0.0), arm_l: [0.2, 0.5, 0.6, 0.0], ..stance };
        let aim = Key { twist: 0.28, bend: 0.06, hip: v3(0.0, -0.07, 0.02), prop: prop(0.3, 0.42, 1.16, 0.0, 0.04, 1.0), feet: brace, ..stance };
        let kick = Key { hip: v3(0.0, -0.08, -0.05), bend: 0.0, prop: prop(0.34, 0.35, 1.18, 0.0, 0.09, 1.0), ..aim };
        clips.push(Clip::new(false, at(46), vec![(0.0, stance, Ease::Out), (at(8), raise, Ease::Smooth), (at(14), aim, Ease::Linear), (at(16), aim, Ease::Out), (at(18), kick, Ease::Smooth), (at(24), aim, Ease::Linear), (at(34), aim, Ease::Smooth), (at(46), stance, Ease::Linear)]));

        // ---- rise: from low on the right to high, leaving the ground
        let low7 = Key { bend: 0.42, twist: 0.62, hip: v3(0.02, -0.22, 0.0), prop: prop(1.5, 0.36, 0.55, 1.9, -0.5, 1.0), ..stance };
        let hop = [foot(-0.07, 0.06, 0.2, -0.05, -0.5), foot(0.09, -0.04, 0.16, 0.2, -0.5)];
        let up7 = Key { bend: -0.16, twist: -0.3, hip: v3(0.0, 0.12, 0.1), head: [0.0, -0.3], prop: prop(0.15, 0.42, 1.42, 0.1, 1.25, 1.0), feet: hop, ..stance };
        let peak7 = Key { hip: v3(0.0, 0.18, 0.1), ..up7 };
        let land7 = Key { hip: v3(0.0, -0.1, 0.08), bend: 0.12, prop: prop(0.4, 0.34, 1.1, -0.2, 0.9, 1.0), ..stance };
        clips.push(Clip::new(false, at(44), vec![(0.0, stance, Ease::Out), (at(6), low7, Ease::In), (at(12), up7, Ease::Out), (at(20), peak7, Ease::Smooth), (at(30), land7, Ease::Smooth), (at(44), stance, Ease::Linear)]));

        // ---- lightning: the staff raised, then brought down toward them
        let high8 = Key { bend: -0.1, twist: 0.2, prop: prop(0.6, 0.25, 1.5, 0.0, 1.52, 0.0), arm_l: [0.2, 0.9, 0.3, 0.0], head: [0.0, -0.2], ..stance };
        let call8 = Key { bend: 0.16, twist: -0.15, hip: v3(0.0, -0.08, 0.06), prop: prop(0.25, 0.5, 1.2, 0.0, 0.25, 0.0), arm_l: [0.6, 0.5, 0.4, 0.0], feet: brace, ..stance };
        let pulse8 = Key { prop: prop(0.25, 0.46, 1.22, 0.0, 0.32, 0.0), hip: v3(0.0, -0.08, 0.03), ..call8 };
        clips.push(Clip::new(
            false,
            at(58),
            vec![(0.0, stance, Ease::Out), (at(10), high8, Ease::In), (at(17), call8, Ease::Out), (at(21), pulse8, Ease::In), (at(24), call8, Ease::Out), (at(28), pulse8, Ease::In), (at(31), call8, Ease::Out), (at(44), call8, Ease::Smooth), (at(58), stance, Ease::Linear)],
        ));

        // ---- fire: a wide swing overhead, the butt driven into the ground
        let wind9 = Key { twist: 0.9, hip: v3(0.02, -0.06, -0.04), prop: prop(2.0, 0.34, 1.2, 2.8, 0.3, 1.0), ..stance };
        let over9 = Key { twist: 0.2, bend: -0.1, hip: v3(0.0, 0.02, 0.0), prop: prop(0.3, 0.25, 1.52, 0.2, 1.3, 1.0), ..stance };
        let plant9 = Key { twist: -0.1, bend: 0.3, hip: v3(0.0, -0.16, 0.1), head: [0.0, -0.25], prop: prop(0.25, 0.42, 0.78, 0.0, 1.5, 0.0), arm_l: [1.4, 0.2, 0.15, 0.0], feet: brace, ..stance };
        clips.push(Clip::new(false, at(66), vec![(0.0, stance, Ease::Out), (at(10), wind9, Ease::Smooth), (at(22), over9, Ease::In), (at(27), plant9, Ease::Out), (at(48), plant9, Ease::Smooth), (at(66), stance, Ease::Linear)]));

        // ---- the volley: she leaves the ground and points
        let hang = [foot(-0.06, 0.03, 0.24, -0.05, -0.55), foot(0.07, -0.05, 0.2, 0.15, -0.6)];
        let float10 = Key { hip: v3(0.0, 0.24, 0.0), bend: -0.08, twist: 0.15, head: [0.0, -0.1], prop: prop(0.5, 0.35, 1.3, -0.9, 0.5, 0.0), arm_l: [0.9, 0.9, 0.2, 0.0], feet: hang, ..stance };
        let point10 = Key { prop: prop(0.3, 0.46, 1.28, 0.0, 0.1, 0.0), arm_l: [0.4, 1.0, 0.25, 0.0], hip: v3(0.0, 0.27, 0.0), ..float10 };
        clips.push(Clip::new(false, at(78), vec![(0.0, stance, Ease::Out), (at(12), float10, Ease::Smooth), (at(20), point10, Ease::Smooth), (at(40), float10, Ease::Smooth), (at(58), point10, Ease::Smooth), (at(68), stance, Ease::Linear), (at(78), stance, Ease::Linear)]));

        // ---- evade: low and fast, the staff trailing
        let dash = Key { lean: 0.55, bend: 0.2, hip: v3(0.0, -0.04, 0.0), head: [0.0, -0.45], prop: prop(1.88, 0.26, 0.84, 2.9, 0.1, 0.0), arm_l: [-0.9, 0.3, 0.3, 0.0], feet: [foot(-0.07, -0.25, 0.12, 0.0, -0.5), foot(0.08, -0.45, 0.2, 0.0, -0.7)], ..stance };
        clips.push(Clip::new(false, at(24), vec![(0.0, stance, Ease::Out), (at(4), dash, Ease::Linear), (at(14), dash, Ease::Smooth), (at(24), stance, Ease::Linear)]));

        // ---- the unsealing: she gathers, head bowed, and lets go
        let gather = Key { hip: v3(0.0, 0.1, 0.0), bend: 0.1, twist: 0.0, head: [0.0, 0.3], prop: prop(0.0, 0.3, 1.1, 0.0, 1.56, 1.0), feet: [foot(-0.06, 0.02, 0.1, -0.05, -0.4), foot(0.07, -0.03, 0.09, 0.12, -0.45)], ..stance };
        let deep = Key { hip: v3(0.0, 0.3, 0.0), head: [0.0, 0.36], feet: hang, ..gather };
        let release = Key { hip: v3(0.0, 0.36, 0.0), bend: -0.2, head: [0.0, -0.36], prop: prop(0.1, 0.25, 1.5, 0.0, 1.56, 0.0), arm_l: [-0.2, 1.3, 0.1, 0.0], feet: hang, ..gather };
        let after = Key { hip: v3(0.0, 0.3, 0.0), bend: -0.1, head: [0.0, -0.1], ..release };
        clips.push(Clip::new(false, at(156), vec![(0.0, stance, Ease::Smooth), (at(20), gather, Ease::Smooth), (at(70), deep, Ease::In), (at(78), release, Ease::Out), (at(130), after, Ease::Smooth), (at(156), stance, Ease::Linear)]));

        let bind_inv = skel.bind_inverse();
        Animator {
            skel,
            bind_inv,
            clips,
            idle,
            stance,
            run,
            hover,
            guard,
            hit,
            down,
            from: ease,
            cur: ease,
            mix: 1.0,
            mix_len: 1.0,
            tag: 0,
            tails: [[V3::ZERO; TAIL_POINTS]; 2],
            tails_prev: [[V3::ZERO; TAIL_POINTS]; 2],
            live: false,
            last_yaw: 0.0,
            world: [M34::ID; BONES],
            skin: [M34::ID; BONES],
            head: V3::ZERO,
            butt: V3::ZERO,
            staff: V3::UP,
        }
    }

    /// After a teleport: nothing carries over.
    pub fn reset(&mut self) {
        self.live = false;
        self.mix = 1.0;
        self.tag = 0;
    }

    /// The key a state asks for, before any cross-fade.
    pub fn want(&self, i: &AnimIn) -> Key {
        match i.show {
            show::MOVE => self.clips[(i.mv as usize).min(self.clips.len() - 1)].sample(i.t / 60.0),
            show::GUARD => self.guard,
            show::HIT => self.hit.sample(i.t / 60.0),
            show::DOWN => self.down.sample(i.t / 60.0),
            _ => {
                let t = i.tick as f32 / 60.0;
                let moving = saturate(i.speed / 2.2);
                let mut k = self.idle.sample(t).blend(&self.run, moving);
                // The staff and the free arm keep time with the legs.
                let s = sin(i.phase);
                k.twist += 0.1 * s * moving;
                k.arm_l[0] += -0.75 * s * moving;
                k.prop.y += 0.02 * cos(i.phase * 2.0) * moving;
                k.prop.a += 0.12 * s * moving;
                gait(&self.skel, &mut k, i.phase, i.speed, moving, 3.0, 6.5);
                if i.hover > 0.001 {
                    let mut h = self.hover;
                    h.hip.y += 0.03 * sin(t * 2.4);
                    h.legs[0][2] += 0.08 * sin(t * 3.1);
                    h.legs[1][2] += 0.08 * sin(t * 2.7 + 1.0);
                    k = k.blend(&h, i.hover);
                }
                k
            }
        }
    }

    pub fn update(&mut self, i: &AnimIn) {
        let dt = 1.0 / 60.0;
        if !i.frozen {
            let tag = (i.show as u32) << 8 | if i.show == show::MOVE { i.mv as u32 } else { 0 };
            if tag != self.tag {
                self.from = self.cur;
                self.mix = 0.0;
                self.mix_len = match i.show {
                    show::MOVE => 3.0,
                    show::HIT => 2.0,
                    show::GUARD => 5.0,
                    _ => 9.0,
                };
                self.tag = tag;
            }
            let want = self.want(i);
            self.mix = min(self.mix + 1.0 / self.mix_len, 1.0);
            let k = self.mix * self.mix * (3.0 - 2.0 * self.mix);
            self.cur = if self.mix >= 1.0 { want } else { self.from.blend(&want, k) };
        }
        let mut w = solve(&self.skel, &self.cur);
        place(&mut w, i.pos, i.yaw);

        // ---- the skirt trails behind her and flares in a turn
        let turn = wrap_angle(i.yaw - self.last_yaw) / dt;
        self.last_yaw = i.yaw;
        if !i.frozen {
            let right = w[PELVIS].r.x;
            let fwd = heading(i.yaw);
            let trail = clamp(i.speed * 0.022, 0.0, 0.34) + 0.02 * sin(i.tick as f32 * 0.5) * saturate(i.speed / 6.0);
            let flare = clamp(abs(turn) * 0.016, 0.0, 0.5);
            for (s, b) in [(-1.0f32, SKIRT_L), (1.0, SKIRT_R)] {
                let q = Quat::axis_angle(right, trail).mul(Quat::axis_angle(fwd, -s * flare));
                w[b].r = q.m3().mul(&w[b].r);
            }
        }

        // ---- hair tails: chains hung from the head
        let seg = self.skel.tail;
        let head_c = w[HEAD].apply(v3(0.0, 0.11, 0.0));
        let (lo, hi) = (w[PELVIS].t, w[CHEST].apply(v3(0.0, 0.16, 0.0)));
        for (side, first) in [(0usize, TAIL_L0), (1, TAIL_R0)] {
            let anchor = w[first].t;
            let p = &mut self.tails[side];
            let prev = &mut self.tails_prev[side];
            if !self.live {
                let back = w[CHEST].r.z;
                for k in 0..TAIL_POINTS {
                    p[k] = anchor + back * [0.0, 0.085, 0.13, 0.135][k] - v3(0.0, seg * k as f32, 0.0);
                    prev[k] = p[k];
                }
            }
            if !i.frozen {
                // Where the tail hangs at rest: down behind the shoulder.
                let back = w[CHEST].r.z;
                for k in 1..TAIL_POINTS {
                    let v = (p[k] - prev[k]) * 0.93;
                    prev[k] = p[k];
                    p[k] = p[k] + v + v3(0.0, -14.0, 0.0) * (dt * dt);
                    let rest = anchor + back * [0.0, 0.085, 0.13, 0.135][k] - V3::UP * (seg * k as f32);
                    p[k] = p[k].lerp(rest, 0.05);
                }
                for _ in 0..4 {
                    p[0] = anchor;
                    for k in 1..TAIL_POINTS {
                        let d = p[k] - p[k - 1];
                        let l = d.len();
                        if l > 1e-5 {
                            p[k] = p[k - 1] + d * (seg / l);
                        }
                        // Out of the head, and out of the trunk.
                        let d = p[k] - head_c;
                        let l = d.len();
                        if l < 0.125 && l > 1e-5 {
                            p[k] = head_c + d * (0.125 / l);
                        }
                        let ab = hi - lo;
                        let t = saturate((p[k] - lo).dot(ab) / ab.len2().max(1e-6));
                        let c = lo + ab * t;
                        let d = p[k] - c;
                        let l = d.len();
                        let r = 0.13 + 0.035 * t;
                        if l < r && l > 1e-5 {
                            p[k] = c + d * (r / l);
                        }
                    }
                }
            }
            let x_ref = w[HEAD].r.x;
            for k in 0..3 {
                let y = (p[k] - p[k + 1]).norm_or(V3::UP);
                let z = x_ref.cross(y).norm_or(v3(0.0, 0.0, 1.0));
                w[first + k] = M34::new(M3 { x: y.cross(z), y, z }, p[k]);
            }
        }
        self.live = true;

        self.staff = w[PROP].r.y;
        self.head = w[PROP].t + self.staff * STAFF_HEAD;
        self.butt = w[PROP].t + self.staff * STAFF_BUTT;
        self.skin = skin(&w, &self.bind_inv);
        self.world = w;
    }
}

impl Default for Animator {
    fn default() -> Self {
        Self::new()
    }
}
