//! Effects as the simulation knows them: a kind, a start tick, a place, a
//! direction and one number. What an effect looks like is authored once
//! (`web/src/fx`) and compiled per device; a renderer evaluates it at the age
//! `tick - t0`, so the simulation stores nothing per particle.
//!
//! The list also says which effects light the world around them.

use crate::math::*;

pub mod kind {
    pub const NONE: u8 = 0;
    /// The crescent a staff sweep leaves. `a` is +1 for right-to-left, -1 for the reverse.
    pub const ARC: u8 = 1;
    /// Sparks where a strike lands on plate.
    pub const SPARK: u8 = 2;
    /// A ring running over the ground from a blow. `a` is its reach.
    pub const SHOCK: u8 = 3;
    /// The band a spinning strike leaves around her.
    pub const WHIRL: u8 = 4;
    /// A magic circle at the staff's head. `a` is its size.
    pub const CIRCLE: u8 = 5;
    /// The burst that leaves the staff on a thrust. `a` is its reach.
    pub const CONE: u8 = 6;
    /// A spell bursting on a target.
    pub const BURST: u8 = 7;
    /// The beam. `a` is its length.
    pub const BEAM: u8 = 8;
    /// A column of light that lifts what stands in it. `a` is its radius.
    pub const PILLAR: u8 = 9;
    /// Forked lightning in a fan. `a` is its reach.
    pub const LIGHTNING: u8 = 10;
    /// Lightning earthing through a knight.
    pub const JOLT: u8 = 11;
    /// A fireball and what burns after. `a` is its radius.
    pub const HELLFIRE: u8 = 12;
    /// Fire on a struck knight.
    pub const EMBER: u8 = 13;
    /// Not drawn: starts a volley of bolts. `a` is how many.
    pub const VOLLEY: u8 = 14;
    /// The streak she leaves when she evades.
    pub const BLINK: u8 = 15;
    /// Mana drawn in toward her before the unsealing. `a` is the radius it comes from.
    pub const GATHER: u8 = 16;
    /// The unsealing: a dome of light that runs outward. `a` is its radius.
    pub const UNSEAL: u8 = 17;
    /// Light rising from a knight whose binding is undone.
    pub const SOUL: u8 = 18;
    /// Dust from a landing body.
    pub const DUST: u8 = 19;
    /// Her barrier, while she guards.
    pub const GUARD: u8 = 20;
    /// A blow stopped by the barrier.
    pub const BLOCK: u8 = 21;
    /// The edge of a knight's weapon through its swing.
    pub const SLASH: u8 = 22;
    /// She is struck.
    pub const HURT: u8 = 23;
    /// A bolt of the volley leaving its circle.
    pub const MUZZLE: u8 = 24;
    pub const COUNT: usize = 25;
}

/// Ticks each kind lives.
pub const LIFE: [u16; kind::COUNT] = [0, 16, 18, 30, 22, 44, 26, 30, 30, 40, 16, 20, 80, 30, 1, 16, 78, 70, 110, 40, 12, 14, 12, 16, 14];

/// What an effect holds on to while it lives.
pub mod follow {
    pub const NONE: u8 = 0;
    /// The staff's head.
    pub const STAFF: u8 = 1;
    /// The mage's centre.
    pub const BODY: u8 = 2;
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Fx {
    pub pos: V3,
    pub dir: V3,
    pub a: f32,
    pub t0: u32,
    pub seed: u32,
    pub kind: u8,
    pub follow: u8,
    pub pad: [u8; 2],
}

pub const SLOTS: usize = 160;

pub struct FxList {
    pub items: [Fx; SLOTS],
    next: usize,
    seed: u32,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct Light {
    pub pos: V3,
    pub radius: f32,
    pub color: [f32; 3],
    pub power: f32,
}

pub const LIGHTS: usize = 4;

impl FxList {
    pub fn new() -> FxList {
        FxList { items: [Fx { pos: V3::ZERO, dir: V3::ZERO, a: 0.0, t0: 0, seed: 0, kind: 0, follow: 0, pad: [0; 2] }; SLOTS], next: 0, seed: 0x9e37_79b9 }
    }

    pub fn clear(&mut self) {
        for f in &mut self.items {
            f.kind = 0;
        }
    }

    pub fn alive(f: &Fx, tick: u32) -> bool {
        f.kind != 0 && tick.wrapping_sub(f.t0) < LIFE[f.kind as usize] as u32
    }

    /// Starts an effect; the slot it takes is one that has ended, or else the oldest.
    pub fn spawn(&mut self, tick: u32, kind: u8, pos: V3, dir: V3, a: f32, follow: u8) {
        let mut slot = self.next;
        for k in 0..SLOTS {
            let i = (self.next + k) % SLOTS;
            if !Self::alive(&self.items[i], tick) {
                slot = i;
                break;
            }
        }
        self.next = (slot + 1) % SLOTS;
        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        self.items[slot] = Fx { pos, dir, a, t0: tick, seed: self.seed, kind, follow, pad: [0; 2] };
    }

    /// The strongest lights among the live effects, seen from `eye`. Returns how many it wrote.
    pub fn lights(&self, tick: u32, eye: V3, out: &mut [Light; LIGHTS]) -> usize {
        let mut score = [0.0f32; LIGHTS];
        let mut n = 0;
        for f in &self.items {
            if !Self::alive(f, tick) {
                continue;
            }
            let age = tick.wrapping_sub(f.t0) as f32 / LIFE[f.kind as usize] as f32;
            let Some(l) = light_of(f, age, tick) else { continue };
            let s = l.power * l.radius / (1.0 + (l.pos - eye).len() * 0.04);
            // Insert by score, keeping the list sorted.
            let mut at = n.min(LIGHTS);
            while at > 0 && score[at - 1] < s {
                at -= 1;
            }
            if at >= LIGHTS {
                continue;
            }
            let last = (n.min(LIGHTS - 1)).max(at);
            let mut k = last;
            while k > at {
                out[k] = out[k - 1];
                score[k] = score[k - 1];
                k -= 1;
            }
            out[at] = l;
            score[at] = s;
            n = (n + 1).min(LIGHTS);
        }
        n
    }
}

impl Default for FxList {
    fn default() -> Self {
        Self::new()
    }
}

/// The light an effect casts at `age` (0 at its start, 1 at its end).
pub fn light_of(f: &Fx, age: f32, tick: u32) -> Option<Light> {
    use kind::*;
    let fade = 1.0 - age;
    let cold = [0.62, 0.78, 1.0];
    let (pos, radius, color, power) = match f.kind {
        ARC | WHIRL => (f.pos + v3(0.0, 1.0, 0.0), 5.0, cold, 0.9 * fade),
        SPARK | BLOCK => (f.pos, 3.5, [1.0, 0.86, 0.62], 1.2 * fade * fade),
        CIRCLE => (f.pos, 5.5, cold, 1.1 * sin(PI * age)),
        CONE => (f.pos + f.dir * 2.5, 9.0, cold, 2.6 * fade),
        BURST => (f.pos + v3(0.0, 0.8, 0.0), 9.0, cold, 2.4 * fade * fade),
        BEAM => (f.pos + f.dir * 7.0, 19.0, [0.7, 0.84, 1.0], 3.4 * fade),
        PILLAR => (f.pos + v3(0.0, 2.0, 0.0), 10.0, cold, 2.4 * fade),
        LIGHTNING => (f.pos + f.dir * 6.0 + v3(0.0, 2.0, 0.0), 24.0, [0.72, 0.74, 1.0], if (tick + f.seed) % 3 == 0 { 1.4 } else { 4.2 } * fade),
        HELLFIRE => (f.pos + v3(0.0, 2.2, 0.0), 24.0, [1.0, 0.42, 0.1], 4.6 * min(1.0, 2.2 * fade)),
        EMBER => (f.pos + v3(0.0, 1.0, 0.0), 4.0, [1.0, 0.5, 0.15], 1.0 * fade),
        GATHER => (f.pos + v3(0.0, 1.2, 0.0), 8.0 + 10.0 * age, cold, 0.6 + 2.2 * age * age),
        UNSEAL => (f.pos + v3(0.0, 3.0, 0.0), 46.0, [0.86, 0.93, 1.0], 6.0 * fade),
        GUARD => (f.pos + f.dir * 0.8 + v3(0.0, 1.0, 0.0), 4.0, cold, 0.7),
        MUZZLE => (f.pos, 5.0, cold, 1.6 * fade),
        _ => return None,
    };
    if power <= 0.01 {
        return None;
    }
    Some(Light { pos, radius, color, power })
}
