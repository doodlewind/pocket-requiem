//! The army: cohorts in formation, and the knights that have left one to
//! fight.
//!
//! A knight in formation costs nothing per tick: its place is its cohort's
//! anchor plus its slot, computed only when something asks. A cohort lets its
//! knights go when the mage comes near; from then each is a free agent that
//! closes in, keeps its distance from its neighbours through a hashed grid,
//! waits its turn in the ring around her, strikes, and answers to being
//! struck. The arrays are laid out by field, so a pass touches only what it
//! needs.

use alloc::vec;
use alloc::vec::Vec;

use crate::field::Field;
use crate::fx::{self, FxList};
use crate::knight::{self, clip};
use crate::math::*;
use crate::moves::{react, Hit, Shape};
use crate::worldfile::Muster;

pub mod state {
    /// In its cohort's formation.
    pub const FORM: u8 = 0;
    /// Closing in, or standing in the ring around her.
    pub const CHASE: u8 = 1;
    pub const ATTACK: u8 = 2;
    pub const STAGGER: u8 = 3;
    pub const KNOCK: u8 = 4;
    pub const AIR: u8 = 5;
    pub const DOWN: u8 = 6;
    pub const RISE: u8 = 7;
    /// Fallen for good: lying, then sinking away.
    pub const DEAD: u8 = 8;
    pub const GONE: u8 = 9;
}

/// A cohort lets go of its knights when she is this near its anchor.
const ENGAGE: f32 = 52.0;
/// A free knight further than this from her stands still.
const LEASH: f32 = 110.0;
/// Knights that may be winding up or striking at once.
const ATTACKERS: u32 = 3;
const GRAVITY: f32 = 22.0;
/// Ticks a cross-fade between two clips takes.
pub const FADE: u8 = 6;
/// A fallen knight lies this long, then sinks for `SINK` ticks.
const LIE: u32 = 170;
const SINK: u32 = 70;
const CELL: f32 = 1.6;
const HASH: usize = 4096;
const BODY: f32 = 0.52;
pub const CAPTAIN_SCALE: f32 = 1.2;

pub struct Cohort {
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
    pub first: u32,
    pub count: u32,
    /// Knights still in formation.
    pub formed: u32,
    /// Cycles of the walk, shared by the formation.
    pub phase: f32,
    pub radius: f32,
    pub marching: bool,
}

/// One knight as a renderer draws it.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct Draw {
    pub pos: V3,
    /// Heading convention.
    pub yaw: f32,
    /// Stored frames to blend, and the blend (0 is all `a`).
    pub a: u16,
    pub b: u16,
    pub blend: f32,
    pub scale: f32,
    /// Squared distance from the eye.
    pub dist2: f32,
    /// 0 to 1: how much of a strike's flash is on it.
    pub flash: f32,
    /// 0 sword, 1 halberd, 2 greatsword.
    pub kind: u8,
    pub pad: [u8; 3],
}

/// A knight's blow landing on the mage.
#[derive(Clone, Copy, Debug)]
pub struct Blow {
    pub damage: f32,
    /// From the knight toward her.
    pub dir: V3,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Struck {
    pub count: u32,
    pub kills: u32,
    /// The freeze the strike asked for.
    pub stop: u32,
}

pub struct Crowd {
    pub n: usize,
    pub x: Vec<f32>,
    pub z: Vec<f32>,
    pub y: Vec<f32>,
    pub yaw: Vec<f32>,
    vx: Vec<f32>,
    vz: Vec<f32>,
    vy: Vec<f32>,
    pub state: Vec<u8>,
    pub clip: Vec<u8>,
    pub kind: Vec<u8>,
    pub big: Vec<u8>,
    /// Tick the clip started (clips that end), cycles (clips that loop).
    pub t0: Vec<u32>,
    pub phase: Vec<f32>,
    from: Vec<u16>,
    fade: Vec<u8>,
    pub hp: Vec<i16>,
    freeze: Vec<u8>,
    flash: Vec<u8>,
    cool: Vec<u16>,
    cohort: Vec<u16>,
    sx: Vec<f32>,
    sz: Vec<f32>,
    pub cohorts: Vec<Cohort>,
    /// Knights out of formation and not yet gone.
    pub free: Vec<u32>,
    cell_start: Vec<u32>,
    cell_items: Vec<u32>,
    cell_fill: Vec<u32>,
    cell_of: Vec<u16>,
    attackers: u32,
    pub fallen: u32,
    pub blows: [Option<Blow>; 4],
    /// Free knights within 30 m of her, for the sound of the press.
    pub near: u32,
}

#[inline]
fn hash(i: u32) -> u32 {
    let mut h = i.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h
}

#[inline]
fn cell_key(x: f32, z: f32) -> usize {
    let ix = floor(x / CELL) as i32;
    let iz = floor(z / CELL) as i32;
    ((ix & 63) | ((iz & 63) << 6)) as usize
}

impl Crowd {
    pub fn new(musters: &[Muster]) -> Crowd {
        let n: usize = musters.iter().map(|m| m.cols as usize * m.rows as usize).sum();
        let mut c = Crowd {
            n,
            x: vec![0.0; n],
            z: vec![0.0; n],
            y: vec![0.0; n],
            yaw: vec![0.0; n],
            vx: vec![0.0; n],
            vz: vec![0.0; n],
            vy: vec![0.0; n],
            state: vec![state::FORM; n],
            clip: vec![clip::IDLE; n],
            kind: vec![0; n],
            big: vec![0; n],
            t0: vec![0; n],
            phase: vec![0.0; n],
            from: vec![0; n],
            fade: vec![0; n],
            hp: vec![60; n],
            freeze: vec![0; n],
            flash: vec![0; n],
            cool: vec![0; n],
            cohort: vec![0; n],
            sx: vec![0.0; n],
            sz: vec![0.0; n],
            cohorts: Vec::with_capacity(musters.len()),
            free: Vec::with_capacity(n),
            cell_start: vec![0; HASH + 1],
            cell_items: Vec::with_capacity(n),
            cell_fill: vec![0; HASH],
            cell_of: vec![0; n],
            attackers: 0,
            fallen: 0,
            blows: [None; 4],
            near: 0,
        };
        let mut at = 0usize;
        for (ci, m) in musters.iter().enumerate() {
            let count = m.cols as usize * m.rows as usize;
            for k in 0..count {
                let i = at + k;
                let (col, row) = ((k % m.cols as usize) as f32, (k / m.cols as usize) as f32);
                let h = hash(i as u32);
                let jitter = |s: u32| ((h >> s) & 255) as f32 / 255.0 - 0.5;
                c.sx[i] = (col - (m.cols as f32 - 1.0) * 0.5) * m.spacing + jitter(0) * 0.5;
                c.sz[i] = row * m.spacing + jitter(8) * 0.5;
                c.cohort[i] = ci as u16;
                c.kind[i] = match m.mix {
                    0..=2 => m.mix,
                    _ => ((h >> 16) % 5).min(2) as u8,
                };
                c.phase[i] = ((h >> 20) & 3) as f32 * 0.25;
                c.yaw[i] = jitter(24) * 0.12;
                if m.captain != 0 && k == m.cols as usize / 2 {
                    c.big[i] = 1;
                    c.kind[i] = 2;
                    c.hp[i] = 340;
                }
            }
            let radius = 0.5 * sqrt((m.cols as f32 * m.spacing) * (m.cols as f32 * m.spacing) + 4.0 * (m.rows as f32 * m.spacing) * (m.rows as f32 * m.spacing)) + 2.0;
            c.cohorts.push(Cohort { x: m.x, z: m.z, yaw: m.yaw, first: at as u32, count: count as u32, formed: count as u32, phase: 0.0, radius, marching: false });
            at += count;
        }
        c
    }

    /// Where knight `i` stands in its formation.
    #[inline]
    fn slot(&self, i: usize) -> (f32, f32) {
        let c = &self.cohorts[self.cohort[i] as usize];
        let (s, co) = (sin(c.yaw), cos(c.yaw));
        // The cohort faces `heading(yaw)`; ranks extend behind the front one.
        (c.x + co * self.sx[i] + s * self.sz[i], c.z - s * self.sx[i] + co * self.sz[i])
    }

    fn set_clip(&mut self, i: usize, to: u8, tick: u32) {
        if self.clip[i] == to && knight::CLIPS[to as usize].looped {
            return;
        }
        let (a, b, t) = self.pair(i, tick);
        self.from[i] = if t < 0.5 { a } else { b };
        self.fade[i] = FADE;
        self.clip[i] = to;
        self.t0[i] = tick;
    }

    /// The stored frames knight `i` shows, before any cross-fade.
    #[inline]
    fn pair(&self, i: usize, tick: u32) -> (u16, u16, f32) {
        let c = self.clip[i];
        if knight::CLIPS[c as usize].looped {
            knight::frames(c, self.phase[i])
        } else {
            knight::frames(c, tick.wrapping_sub(self.t0[i]) as f32 / 60.0)
        }
    }

    fn release(&mut self, ci: usize, field: &Field, tick: u32) {
        let (first, count) = (self.cohorts[ci].first as usize, self.cohorts[ci].count as usize);
        for i in first..first + count {
            if self.state[i] != state::FORM {
                continue;
            }
            let (x, z) = self.slot(i);
            self.x[i] = x;
            self.z[i] = z;
            self.y[i] = field.height(x, z);
            self.yaw[i] += self.cohorts[ci].yaw;
            if self.cohorts[ci].marching {
                self.phase[i] += self.cohorts[ci].phase;
            }
            self.state[i] = state::CHASE;
            // The ranks do not all step off at once.
            self.cool[i] = (hash(i as u32 ^ tick) % 50) as u16 + (self.sz[i] * 6.0) as u16;
            self.free.push(i as u32);
        }
        self.cohorts[ci].formed = 0;
    }

    /// One tick. `target` is the mage.
    pub fn tick(&mut self, field: &Field, tick: u32, target: V3, fx: &mut FxList) {
        let dt = 1.0 / 60.0;
        self.blows = [None; 4];
        // ---- cohorts: march on her when she is in sight, let go when she is near
        for ci in 0..self.cohorts.len() {
            if self.cohorts[ci].formed == 0 {
                continue;
            }
            let c = &mut self.cohorts[ci];
            let (dx, dz) = (target.x - c.x, target.z - c.z);
            let d = sqrt(dx * dx + dz * dz);
            if d < ENGAGE + c.radius * 0.5 {
                self.release(ci, field, tick);
                continue;
            }
            c.marching = d < 260.0;
            if c.marching {
                let want = atan2(-dx, -dz);
                c.yaw = wrap_angle(c.yaw + clamp(wrap_angle(want - c.yaw), -0.12 * dt, 0.12 * dt));
                let f = heading(c.yaw);
                c.x += f.x * knight::WALK_SPEED * dt;
                c.z += f.z * knight::WALK_SPEED * dt;
                c.phase += knight::WALK_SPEED * dt / knight::stride_of(1, false);
                if c.phase >= 1.0 {
                    c.phase -= 1.0;
                }
            }
        }

        // ---- the grid over standing free knights: a counting sort by hashed cell
        for s in self.cell_start.iter_mut() {
            *s = 0;
        }
        for &i in &self.free {
            let i = i as usize;
            if self.state[i] <= state::KNOCK {
                let k = cell_key(self.x[i], self.z[i]);
                self.cell_of[i] = k as u16;
                self.cell_start[k + 1] += 1;
            }
        }
        for k in 0..HASH {
            self.cell_start[k + 1] += self.cell_start[k];
        }
        self.cell_items.clear();
        self.cell_items.resize(self.cell_start[HASH] as usize, 0);
        self.cell_fill.copy_from_slice(&self.cell_start[..HASH]);
        for &i in &self.free {
            if self.state[i as usize] <= state::KNOCK {
                let k = self.cell_of[i as usize] as usize;
                self.cell_items[self.cell_fill[k] as usize] = i;
                self.cell_fill[k] += 1;
            }
        }

        let mut attackers = 0u32;
        let mut near = 0u32;
        let mut gone = false;
        let mut blow = 0usize;
        for fi in 0..self.free.len() {
            let i = self.free[fi] as usize;
            if self.flash[i] > 0 {
                self.flash[i] -= 1;
            }
            if self.fade[i] > 0 {
                self.fade[i] -= 1;
            }
            if self.freeze[i] > 0 {
                // Held by a strike: its clip does not advance either.
                self.freeze[i] -= 1;
                self.t0[i] = self.t0[i].wrapping_add(1);
                if self.state[i] == state::ATTACK {
                    attackers += 1;
                }
                continue;
            }
            let (dx, dz) = (target.x - self.x[i], target.z - self.z[i]);
            let d2 = dx * dx + dz * dz;
            if d2 < 900.0 {
                near += 1;
            }
            let since = tick.wrapping_sub(self.t0[i]);
            match self.state[i] {
                state::CHASE => {
                    if d2 > LEASH * LEASH {
                        self.set_clip(i, clip::IDLE, tick);
                        continue;
                    }
                    let d = sqrt(d2).max(1e-3);
                    let reach = knight::REACH[self.kind[i] as usize] * if self.big[i] != 0 { CAPTAIN_SCALE } else { 1.0 };
                    let want = atan2(-dx, -dz);
                    self.yaw[i] = wrap_angle(self.yaw[i] + clamp(wrap_angle(want - self.yaw[i]), -5.0 * dt, 5.0 * dt));
                    if self.cool[i] > 0 {
                        self.cool[i] -= 1;
                    }
                    // A knight whose turn has not come stands off in a loose ring, each at its own distance;
                    // one whose turn has come steps in to its weapon's reach.
                    let waiting = self.cool[i] > 0 || self.attackers >= ATTACKERS;
                    let ring = if waiting { 3.2 + (hash(i as u32) & 255) as f32 / 255.0 * 3.6 } else { reach * 0.8 };
                    let mut speed = 0.0;
                    if d > ring + 0.3 {
                        speed = if d > 9.0 { knight::RUN_SPEED } else { knight::WALK_SPEED * 1.2 };
                        if self.big[i] != 0 {
                            speed *= 0.85;
                        }
                    } else if d < ring - 0.5 {
                        speed = -knight::WALK_SPEED * 0.7;
                    } else if !waiting && target.y - self.y[i] < 2.2 {
                        let which = if hash(i as u32 ^ tick.wrapping_mul(31)) & 1 == 0 { clip::CHOP } else { clip::SWEEP };
                        self.state[i] = state::ATTACK;
                        self.set_clip(i, which, tick);
                        self.attackers += 1;
                        attackers += 1;
                        continue;
                    }
                    let (mut mx, mut mz) = (dx / d * speed * dt, dz / d * speed * dt);
                    // Keep off the neighbours: the eight cells around and its own.
                    let (px, pz) = (self.x[i], self.z[i]);
                    let (cx, cz) = (floor(px / CELL) as i32, floor(pz / CELL) as i32);
                    let mut pushed = 0.0f32;
                    for oz in -1..=1 {
                        for ox in -1..=1 {
                            let k = (((cx + ox) & 63) | (((cz + oz) & 63) << 6)) as usize;
                            for &j in &self.cell_items[self.cell_start[k] as usize..self.cell_start[k + 1] as usize] {
                                let j = j as usize;
                                if j == i {
                                    continue;
                                }
                                let (ex, ez) = (px - self.x[j], pz - self.z[j]);
                                let e2 = ex * ex + ez * ez;
                                let gap = BODY * 2.0 * if self.big[i] | self.big[j] != 0 { 1.25 } else { 1.0 };
                                if e2 < gap * gap && e2 > 1e-6 {
                                    let e = sqrt(e2);
                                    let k = (gap - e) / e * 0.35;
                                    mx += ex * k;
                                    mz += ez * k;
                                    pushed += gap - e;
                                }
                            }
                        }
                    }
                    // Blocked from ahead: slide around.
                    if pushed > 0.25 && speed > 0.0 {
                        let side = if hash(i as u32) & 1 == 0 { 1.0 } else { -1.0 };
                        mx += -dz / d * side * knight::WALK_SPEED * dt;
                        mz += dx / d * side * knight::WALK_SPEED * dt;
                    }
                    // She is solid too.
                    if d < 0.85 {
                        mx -= dx / d * (0.85 - d);
                        mz -= dz / d * (0.85 - d);
                    }
                    self.x[i] += mx;
                    self.z[i] += mz;
                    field.push_out(&mut self.x[i], &mut self.z[i], BODY);
                    self.y[i] = field.height(self.x[i], self.z[i]);
                    let moved = sqrt(mx * mx + mz * mz) / dt;
                    if moved > 0.5 {
                        let run = moved > 2.8;
                        self.set_clip(i, if run { clip::RUN } else { clip::WALK }, tick);
                        let p = self.phase[i] + moved * dt / knight::stride_of(self.kind[i] as u32 + 1, run);
                        self.phase[i] = p - floor(p);
                    } else {
                        self.set_clip(i, clip::IDLE, tick);
                        let p = self.phase[i] + dt / knight::CLIPS[0].len;
                        self.phase[i] = p - floor(p);
                    }
                }
                state::ATTACK => {
                    attackers += 1;
                    let hit_at = if self.clip[i] == clip::CHOP { knight::CHOP_HIT } else { knight::SWEEP_HIT };
                    if since < hit_at - 8 {
                        // It still tracks her while it winds up.
                        let want = atan2(-dx, -dz);
                        self.yaw[i] = wrap_angle(self.yaw[i] + clamp(wrap_angle(want - self.yaw[i]), -2.5 * dt, 2.5 * dt));
                    }
                    if since == hit_at {
                        let f = heading(self.yaw[i]);
                        let d = sqrt(d2).max(1e-3);
                        let reach = knight::REACH[self.kind[i] as usize] * if self.big[i] != 0 { CAPTAIN_SCALE } else { 1.0 };
                        fx.spawn(tick, fx::kind::SLASH, v3(self.x[i], self.y[i] + 1.1, self.z[i]), f, reach, 0);
                        if d < reach + 0.4 && (dx * f.x + dz * f.z) / d > 0.45 && blow < 4 && abs(target.y - self.y[i]) < 2.4 {
                            let damage = [6.0, 8.0, 11.0][self.kind[i] as usize] * if self.big[i] != 0 { 2.0 } else { 1.0 };
                            self.blows[blow] = Some(Blow { damage, dir: v3(dx / d, 0.0, dz / d) });
                            blow += 1;
                        }
                    }
                    if since >= 78 {
                        self.state[i] = state::CHASE;
                        self.cool[i] = 150 + (hash(i as u32 ^ tick) % 330) as u16;
                    }
                }
                state::STAGGER | state::KNOCK => {
                    self.x[i] += self.vx[i] * dt;
                    self.z[i] += self.vz[i] * dt;
                    let k = exp(-5.0 * dt);
                    self.vx[i] *= k;
                    self.vz[i] *= k;
                    field.push_out(&mut self.x[i], &mut self.z[i], BODY);
                    self.y[i] = field.height(self.x[i], self.z[i]);
                    if since >= if self.state[i] == state::STAGGER { 27 } else { 48 } {
                        self.state[i] = state::CHASE;
                    }
                }
                state::AIR => {
                    self.vy[i] -= GRAVITY * dt;
                    self.x[i] += self.vx[i] * dt;
                    self.z[i] += self.vz[i] * dt;
                    self.y[i] += self.vy[i] * dt;
                    let p = self.phase[i] + dt / knight::CLIPS[clip::AIR as usize].len;
                    self.phase[i] = p - floor(p);
                    field.push_out(&mut self.x[i], &mut self.z[i], BODY);
                    let ground = field.height(self.x[i], self.z[i]);
                    if self.y[i] <= ground && self.vy[i] < 0.0 {
                        self.y[i] = ground;
                        self.vx[i] *= 0.3;
                        self.vz[i] *= 0.3;
                        self.state[i] = if self.hp[i] <= 0 { state::DEAD } else { state::DOWN };
                        self.set_clip(i, clip::DOWN, tick);
                        if d2 < 3600.0 {
                            fx.spawn(tick, fx::kind::DUST, v3(self.x[i], ground, self.z[i]), V3::UP, 1.0, 0);
                        }
                    }
                }
                state::DOWN => {
                    self.x[i] += self.vx[i] * dt;
                    self.z[i] += self.vz[i] * dt;
                    let k = exp(-8.0 * dt);
                    self.vx[i] *= k;
                    self.vz[i] *= k;
                    self.y[i] = field.height(self.x[i], self.z[i]);
                    if since >= 70 {
                        self.state[i] = state::RISE;
                        self.set_clip(i, clip::RISE, tick);
                    }
                }
                state::RISE => {
                    if since >= 66 {
                        self.state[i] = state::CHASE;
                        self.cool[i] = 40;
                    }
                }
                state::DEAD => {
                    if self.clip[i] == clip::DOWN {
                        self.x[i] += self.vx[i] * dt;
                        self.z[i] += self.vz[i] * dt;
                        let k = exp(-8.0 * dt);
                        self.vx[i] *= k;
                        self.vz[i] *= k;
                        self.y[i] = field.height(self.x[i], self.z[i]);
                    }
                    if since == 24 && d2 < 4900.0 {
                        fx.spawn(tick, fx::kind::SOUL, v3(self.x[i], self.y[i] + 0.3, self.z[i]), V3::UP, 1.0, 0);
                    }
                    if since >= LIE + SINK {
                        self.state[i] = state::GONE;
                        gone = true;
                    }
                }
                _ => {}
            }
        }
        self.attackers = attackers;
        self.near = near;
        if gone {
            let state = &self.state;
            self.free.retain(|&i| state[i as usize] != state::GONE);
        }
    }

    /// Applies a strike whose shape sits at `origin` facing `yaw`. Returns what it struck.
    pub fn strike(&mut self, hit: &Hit, origin: V3, yaw: f32, tick: u32, fx: &mut FxList) -> Struck {
        let f = heading(yaw);
        let mut out = Struck::default();
        for fi in 0..self.free.len() {
            let i = self.free[fi] as usize;
            let s = self.state[i];
            if s == state::DEAD || s == state::GONE {
                continue;
            }
            let (dx, dz) = (self.x[i] - origin.x, self.z[i] - origin.z);
            let d2 = dx * dx + dz * dz;
            let dy = self.y[i] - origin.y;
            let inside = match hit.shape {
                Shape::Arc { r, half } => d2 < r * r && dy > -3.0 && dy < 5.0 && (d2 < 0.36 || (dx * f.x + dz * f.z) >= cos(half) * sqrt(d2)),
                Shape::Line { len, w } => {
                    let along = dx * f.x + dz * f.z;
                    let across = dx * -f.z + dz * f.x;
                    along > -0.5 && along < len && abs(across) < w + BODY && dy > -4.0 && dy < 6.0
                }
                Shape::Ring { r } => d2 < r * r,
                Shape::Blast { ahead, r } => {
                    let (ex, ez) = (dx - f.x * ahead, dz - f.z * ahead);
                    ex * ex + ez * ez < r * r && dy > -4.0 && dy < 7.0
                }
            };
            if !inside {
                continue;
            }
            // A knight in the air or on the ground is struck by what lifts; a sweep passes over one lying down.
            if (s == state::DOWN || s == state::RISE) && hit.react == react::LIGHT {
                continue;
            }
            let d = sqrt(d2).max(1e-3);
            let (ax, az) = match hit.shape {
                Shape::Blast { ahead, .. } => {
                    let (ex, ez) = (dx - f.x * ahead, dz - f.z * ahead);
                    let e = sqrt(ex * ex + ez * ez);
                    if e > 0.3 {
                        (ex / e, ez / e)
                    } else {
                        (f.x, f.z)
                    }
                }
                Shape::Line { .. } => (f.x, f.z),
                _ => (dx / d, dz / d),
            };
            let big = self.big[i] != 0;
            let damage = if hit.react == react::DISPEL && big { 360 } else { hit.damage };
            // A knight already undone and still in the air can be struck again; it is counted once.
            let stood = self.hp[i] > 0;
            self.hp[i] = self.hp[i].saturating_sub(damage);
            let dead = self.hp[i] <= 0;
            out.count += 1;
            self.flash[i] = 10;
            self.yaw[i] = atan2(ax, az);
            let mut react = hit.react;
            if big && !dead {
                // A captain keeps its feet until it is spent.
                react = match react {
                    react::LIGHT => 255,
                    react::LAUNCH => react::HEAVY,
                    r => r,
                };
            }
            if dead && react == react::LIGHT {
                react = react::DISPEL;
            }
            if react == react::DISPEL && !dead {
                react = react::HEAVY;
            }
            let jitter = 0.8 + (hash(i as u32 ^ tick) & 255) as f32 / 640.0;
            match react {
                react::LIGHT => {
                    self.state[i] = state::STAGGER;
                    self.set_clip(i, clip::STAGGER, tick);
                    self.vx[i] = ax * hit.push * jitter;
                    self.vz[i] = az * hit.push * jitter;
                }
                react::HEAVY => {
                    if dead {
                        self.state[i] = state::AIR;
                        self.set_clip(i, clip::AIR, tick);
                        self.vx[i] = ax * (hit.push + 2.0) * jitter;
                        self.vz[i] = az * (hit.push + 2.0) * jitter;
                        self.vy[i] = 4.5;
                        self.y[i] += 0.05;
                    } else {
                        self.state[i] = state::KNOCK;
                        self.set_clip(i, clip::KNOCK, tick);
                        self.vx[i] = ax * (hit.push + 3.0) * jitter;
                        self.vz[i] = az * (hit.push + 3.0) * jitter;
                    }
                }
                react::LAUNCH => {
                    self.state[i] = state::AIR;
                    self.set_clip(i, clip::AIR, tick);
                    self.vx[i] = ax * hit.push * jitter;
                    self.vz[i] = az * hit.push * jitter;
                    self.vy[i] = hit.lift * (0.85 + (hash(i as u32 ^ tick ^ 77) & 255) as f32 / 850.0);
                    self.y[i] += 0.05;
                }
                react::DISPEL => {
                    self.state[i] = state::DEAD;
                    self.set_clip(i, clip::COLLAPSE, tick);
                    self.vx[i] = 0.0;
                    self.vz[i] = 0.0;
                }
                _ => {}
            }
            // The freeze: the knight holds its frame as long as she does, a wave of undoing a little longer the further out.
            self.freeze[i] = if hit.react == react::DISPEL { (hit.stop as f32 + d * 1.6) as u8 } else { hit.stop };
            if dead && stood {
                out.kills += 1;
                self.fallen += 1;
            }
            if hit.fx != 0 && out.count <= 24 {
                fx.spawn(tick, hit.fx, v3(self.x[i], self.y[i] + 1.15, self.z[i]), v3(ax, 0.3, az), 1.0, 0);
            }
        }
        out.stop = if out.count == 0 { 0 } else { hit.stop as u32 + if out.count >= 5 { 2 } else { 0 } + if out.kills > 0 { 1 } else { 0 } };
        out
    }

    /// The nearest standing free knight within `reach` of `p` and within `half` radians of `yaw`: where a strike should turn to.
    pub fn nearest(&self, p: V3, yaw: f32, reach: f32, half: f32) -> Option<(f32, f32)> {
        let f = heading(yaw);
        let mut best: Option<(f32, f32, f32)> = None;
        for &i in &self.free {
            let i = i as usize;
            if self.state[i] >= state::DEAD {
                continue;
            }
            let (dx, dz) = (self.x[i] - p.x, self.z[i] - p.z);
            let d2 = dx * dx + dz * dz;
            if d2 > reach * reach || d2 < 1e-4 {
                continue;
            }
            let d = sqrt(d2);
            let facing = (dx * f.x + dz * f.z) / d;
            if facing < cos(half) {
                continue;
            }
            let score = d * (1.6 - facing);
            if best.map_or(true, |b| score < b.2) {
                best = Some((self.x[i], self.z[i], score));
            }
        }
        best.map(|b| (b.0, b.1))
    }

    /// Standing free knights within `r` of `p`.
    pub fn count_near(&self, p: V3, r: f32) -> u32 {
        let mut n = 0;
        for &i in &self.free {
            let i = i as usize;
            if self.state[i] >= state::DOWN {
                continue;
            }
            let (dx, dz) = (self.x[i] - p.x, self.z[i] - p.z);
            if dx * dx + dz * dz < r * r {
                n += 1;
            }
        }
        n
    }

    /// The centre of the nearest cohort still in formation, or of the free knights when none is left.
    pub fn nearest_host(&self, p: V3) -> Option<(f32, f32)> {
        let mut best: Option<(f32, f32, f32)> = None;
        for c in &self.cohorts {
            if c.formed == 0 {
                continue;
            }
            let d2 = (c.x - p.x) * (c.x - p.x) + (c.z - p.z) * (c.z - p.z);
            if best.map_or(true, |b| d2 < b.2) {
                best = Some((c.x, c.z, d2));
            }
        }
        best.map(|b| (b.0, b.1))
    }

    /// Knights that are neither fallen nor gone.
    pub fn standing(&self) -> u32 {
        self.n as u32 - self.fallen
    }

    /// Writes every knight inside the frustum `planes` (`n·p + d >= 0` inside) into `out`.
    pub fn draw(&self, field: &Field, tick: u32, planes: &[[f32; 4]; 6], eye: V3, far: f32, out: &mut Vec<Draw>) {
        out.clear();
        let inside = |p: V3, r: f32| {
            for pl in &planes[..4] {
                if pl[0] * p.x + pl[1] * p.y + pl[2] * p.z + pl[3] < -r {
                    return false;
                }
            }
            true
        };
        let far2 = far * far;
        for c in &self.cohorts {
            if c.formed == 0 {
                continue;
            }
            let centre = v3(c.x, field.height(c.x, c.z) + 1.0, c.z);
            if (centre - eye).len() > far + c.radius || !inside(centre, c.radius + 3.0) {
                continue;
            }
            for i in c.first as usize..(c.first + c.count) as usize {
                if self.state[i] != state::FORM {
                    continue;
                }
                let (x, z) = self.slot(i);
                let p = v3(x, field.height(x, z), z);
                let dist2 = (p - eye).len2();
                if dist2 > far2 || !inside(p + v3(0.0, 1.0, 0.0), 1.6) {
                    continue;
                }
                let (a, b, blend) = if c.marching {
                    let u = c.phase + self.phase[i];
                    knight::frames(clip::WALK, u)
                } else {
                    // Standing ranks sway out of step.
                    knight::frames(clip::IDLE, tick as f32 / (60.0 * knight::CLIPS[0].len) + self.phase[i] + (hash(i as u32) & 255) as f32 / 255.0)
                };
                let big = self.big[i] != 0;
                out.push(Draw { pos: p, yaw: c.yaw + self.yaw[i], a, b, blend, scale: if big { CAPTAIN_SCALE } else { 1.0 }, dist2, flash: 0.0, kind: self.kind[i], pad: [0; 3] });
            }
        }
        for &i in &self.free {
            let i = i as usize;
            let mut p = v3(self.x[i], self.y[i], self.z[i]);
            let dist2 = (p - eye).len2();
            // A knight the eye stands in would fill the frame.
            if dist2 > far2 || dist2 < 2.4 * 2.4 || !inside(p + v3(0.0, 1.0, 0.0), 2.2) {
                continue;
            }
            let (mut a, mut b, mut blend) = self.pair(i, tick);
            if self.fade[i] > 0 {
                // From the frame it left to the frame it is at.
                b = if blend < 0.5 { a } else { b };
                a = self.from[i];
                blend = 1.0 - self.fade[i] as f32 / FADE as f32;
            }
            if self.state[i] == state::DEAD {
                let since = tick.wrapping_sub(self.t0[i]);
                if since > LIE {
                    p.y -= (since - LIE) as f32 / SINK as f32 * 0.7;
                }
            }
            if self.freeze[i] > 0 {
                // Held in a strike's freeze: a shiver.
                let h = hash(i as u32 ^ tick.wrapping_mul(7919));
                p.x += ((h & 255) as f32 / 255.0 - 0.5) * 0.07;
                p.z += (((h >> 8) & 255) as f32 / 255.0 - 0.5) * 0.07;
            }
            let big = self.big[i] != 0;
            out.push(Draw { pos: p, yaw: self.yaw[i], a, b, blend, scale: if big { CAPTAIN_SCALE } else { 1.0 }, dist2, flash: self.flash[i] as f32 / 10.0, kind: self.kind[i], pad: [0; 3] });
        }
    }
}
