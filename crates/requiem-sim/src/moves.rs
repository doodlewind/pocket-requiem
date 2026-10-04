//! The mage's moves as data: frame counts at 60 ticks a second, what each
//! strike hits and how hard, when the next input may take over, and which
//! move a light or a heavy input leads to.
//!
//! A chain is five light strikes of the staff. A heavy input after `n` light
//! strikes casts the spell of that step; a heavy input on its own casts
//! the beam. The animation of a move is the clip of the same index
//! (`mage.rs`); the hit shapes here are in the mage's frame, ahead being
//! the way she faces.

/// Move ids. 0 is no move.
pub mod mv {
    pub const NONE: u8 = 0;
    pub const L1: u8 = 1;
    pub const L2: u8 = 2;
    pub const L3: u8 = 3;
    pub const L4: u8 = 4;
    pub const L5: u8 = 5;
    /// A beam.
    pub const BEAM: u8 = 6;
    /// A rising burst that lifts what stands around her.
    pub const RISE: u8 = 7;
    /// Lightning in a fan.
    pub const LIGHTNING: u8 = 8;
    /// Fire that bursts ahead of her.
    pub const HELLFIRE: u8 = 9;
    /// A volley of homing bolts.
    pub const BARRAGE: u8 = 10;
    pub const EVADE: u8 = 11;
    /// Her mana unsealed: the spell that holds the dead army comes undone around her.
    pub const UNSEAL: u8 = 12;
    pub const COUNT: usize = 13;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// A sector of radius `r`, `half` radians to each side of ahead.
    Arc { r: f32, half: f32 },
    /// A lane ahead: `len` long, `w` to each side.
    Line { len: f32, w: f32 },
    /// A disc around her.
    Ring { r: f32 },
    /// A disc of radius `r`, `ahead` metres in front.
    Blast { ahead: f32, r: f32 },
}

/// How a struck knight answers.
pub mod react {
    /// Rocks back and recovers.
    pub const LIGHT: u8 = 0;
    /// Staggers back several steps.
    pub const HEAVY: u8 = 1;
    /// Leaves the ground.
    pub const LAUNCH: u8 = 2;
    /// The spell on it is undone: it falls where it stands.
    pub const DISPEL: u8 = 3;
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    /// Tick of the move at which it lands.
    pub at: u16,
    pub shape: Shape,
    pub damage: i16,
    /// Speed given along the ground, away from her, and upward.
    pub push: f32,
    pub lift: f32,
    /// Ticks the strike freezes her and what it struck.
    pub stop: u8,
    pub react: u8,
    /// Effect at each struck knight, and camera shake.
    pub fx: u8,
    pub shake: f32,
}

/// Something a move starts at a tick: an effect, or bolts.
#[derive(Clone, Copy, Debug)]
pub struct Cue {
    pub at: u16,
    pub fx: u8,
    /// Meaning depends on the effect: a reach, a radius, a count.
    pub a: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Move {
    pub name: &'static str,
    /// Ticks from start to the end of recovery.
    pub len: u16,
    /// First tick at which a buffered input may start the next move.
    pub cancel: u16,
    pub hits: &'static [Hit],
    pub cues: &'static [Cue],
    /// She moves ahead at `speed` from tick `from` to tick `to`.
    pub lunge: (u16, u16, f32),
    /// Ticks from the start during which she still turns to the stick or to the nearest knight.
    pub turn: u16,
    pub next_light: u8,
    pub next_heavy: u8,
    /// Ticks from `0` to `1` during which nothing can hurt her.
    pub safe: (u16, u16),
    /// A full turn of the body between these ticks (the spinning strike).
    pub spin: (u16, u16),
}

use crate::fx::kind as fx;
use mv::*;
use react::*;
use Shape::*;

const fn hit(at: u16, shape: Shape, damage: i16, push: f32, lift: f32, stop: u8, react: u8, fx: u8, shake: f32) -> Hit {
    Hit { at, shape, damage, push, lift, stop, react, fx, shake }
}
const fn cue(at: u16, fx: u8, a: f32) -> Cue {
    Cue { at, fx, a }
}

const NO_MOVE: Move = Move { name: "", len: 1, cancel: 0, hits: &[], cues: &[], lunge: (0, 0, 0.0), turn: 0, next_light: L1, next_heavy: BEAM, safe: (0, 0), spin: (0, 0) };

pub static MOVES: [Move; mv::COUNT] = [
    NO_MOVE,
    Move {
        name: "sweep",
        len: 30,
        cancel: 16,
        hits: &[hit(10, Arc { r: 3.1, half: 1.35 }, 34, 2.6, 0.0, 3, LIGHT, fx::SPARK, 0.12)],
        cues: &[cue(8, fx::ARC, 1.0)],
        lunge: (4, 11, 4.5),
        turn: 8,
        next_light: L2,
        next_heavy: RISE,
        ..NO_MOVE
    },
    Move {
        name: "backsweep",
        len: 30,
        cancel: 15,
        hits: &[hit(9, Arc { r: 3.1, half: 1.35 }, 34, 2.6, 0.0, 3, LIGHT, fx::SPARK, 0.12)],
        cues: &[cue(7, fx::ARC, -1.0)],
        lunge: (3, 10, 4.5),
        turn: 7,
        next_light: L3,
        next_heavy: LIGHTNING,
        ..NO_MOVE
    },
    Move {
        name: "smash",
        len: 36,
        cancel: 21,
        hits: &[hit(13, Blast { ahead: 1.7, r: 2.6 }, 48, 2.0, 0.0, 5, HEAVY, fx::SPARK, 0.22)],
        cues: &[cue(13, fx::SHOCK, 2.6)],
        lunge: (6, 13, 5.0),
        turn: 10,
        next_light: L4,
        next_heavy: HELLFIRE,
        ..NO_MOVE
    },
    Move {
        name: "whirl",
        len: 40,
        cancel: 25,
        hits: &[hit(10, Ring { r: 3.4 }, 30, 3.0, 0.0, 3, LIGHT, fx::SPARK, 0.12), hit(19, Ring { r: 3.4 }, 30, 4.0, 0.0, 4, HEAVY, fx::SPARK, 0.16)],
        cues: &[cue(7, fx::WHIRL, 3.4)],
        lunge: (4, 20, 2.5),
        turn: 6,
        next_light: L5,
        next_heavy: BARRAGE,
        spin: (6, 22),
        ..NO_MOVE
    },
    Move {
        name: "thrust",
        len: 54,
        cancel: 40,
        hits: &[hit(18, Arc { r: 7.0, half: 0.62 }, 72, 10.0, 5.5, 8, LAUNCH, fx::BURST, 0.4)],
        cues: &[cue(12, fx::CIRCLE, 0.8), cue(18, fx::CONE, 7.0)],
        lunge: (10, 18, 7.0),
        turn: 12,
        next_light: NONE,
        next_heavy: NONE,
        ..NO_MOVE
    },
    Move {
        name: "beam",
        len: 46,
        cancel: 34,
        hits: &[hit(16, Line { len: 30.0, w: 1.2 }, 90, 7.0, 3.5, 6, LAUNCH, fx::BURST, 0.35)],
        cues: &[cue(4, fx::CIRCLE, 1.1), cue(16, fx::BEAM, 30.0)],
        lunge: (0, 0, 0.0),
        turn: 14,
        next_light: NONE,
        next_heavy: NONE,
        ..NO_MOVE
    },
    Move {
        name: "rise",
        len: 44,
        cancel: 28,
        hits: &[hit(12, Blast { ahead: 1.5, r: 3.2 }, 50, 1.2, 9.0, 5, LAUNCH, fx::SPARK, 0.25)],
        cues: &[cue(11, fx::PILLAR, 3.2)],
        lunge: (5, 12, 4.0),
        turn: 9,
        next_light: NONE,
        next_heavy: NONE,
        ..NO_MOVE
    },
    Move {
        name: "lightning",
        len: 58,
        cancel: 44,
        hits: &[
            hit(18, Arc { r: 15.0, half: 0.7 }, 36, 3.0, 0.0, 3, HEAVY, fx::JOLT, 0.2),
            hit(25, Arc { r: 15.0, half: 0.7 }, 36, 3.0, 0.0, 3, HEAVY, fx::JOLT, 0.2),
            hit(32, Arc { r: 15.0, half: 0.7 }, 44, 8.0, 4.5, 7, LAUNCH, fx::JOLT, 0.4),
        ],
        cues: &[cue(8, fx::CIRCLE, 1.0), cue(17, fx::LIGHTNING, 15.0), cue(24, fx::LIGHTNING, 15.0), cue(31, fx::LIGHTNING, 15.0)],
        lunge: (0, 0, 0.0),
        turn: 16,
        next_light: NONE,
        next_heavy: NONE,
        ..NO_MOVE
    },
    Move {
        name: "hellfire",
        len: 66,
        cancel: 50,
        hits: &[hit(27, Blast { ahead: 7.0, r: 7.5 }, 120, 11.0, 7.5, 10, LAUNCH, fx::EMBER, 0.6)],
        cues: &[cue(8, fx::CIRCLE, 1.3), cue(26, fx::HELLFIRE, 7.5)],
        lunge: (0, 0, 0.0),
        turn: 22,
        next_light: NONE,
        next_heavy: NONE,
        ..NO_MOVE
    },
    Move {
        name: "barrage",
        len: 78,
        cancel: 62,
        hits: &[],
        cues: &[cue(6, fx::CIRCLE, 1.0), cue(16, fx::VOLLEY, 10.0)],
        lunge: (0, 0, 0.0),
        turn: 50,
        next_light: NONE,
        next_heavy: NONE,
        safe: (10, 50),
        ..NO_MOVE
    },
    Move { name: "evade", len: 24, cancel: 15, hits: &[], cues: &[cue(0, fx::BLINK, 1.0)], lunge: (0, 13, 15.0), turn: 1, next_light: L1, next_heavy: BEAM, safe: (1, 16), spin: (0, 0) },
    Move {
        name: "unseal",
        len: 156,
        cancel: 150,
        hits: &[hit(78, Ring { r: 26.0 }, 30000, 0.0, 0.0, 16, DISPEL, fx::SOUL, 1.0)],
        cues: &[cue(0, fx::GATHER, 26.0), cue(76, fx::UNSEAL, 26.0)],
        lunge: (0, 0, 0.0),
        turn: 0,
        next_light: NONE,
        next_heavy: NONE,
        safe: (0, 156),
        spin: (0, 0),
    },
];

/// What the bolts of a volley do where they land.
pub const BOLT_HIT: Hit = hit(0, Blast { ahead: 0.0, r: 3.2 }, 60, 5.0, 5.0, 2, LAUNCH, fx::BURST, 0.22);
/// Ticks between two bolts of a volley.
pub const BOLT_EVERY: u32 = 4;
