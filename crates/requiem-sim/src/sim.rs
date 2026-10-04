//! One tick of the game: the mage's state, her strikes landing on the army,
//! the freeze a strike puts on both sides, the army's answer, the bolts in
//! the air and the camera.
//!
//! A tick is 1/60 s on every device, whatever its frame rate.

use crate::crowd::{Crowd, Struck};
use crate::field::Field;
use crate::mage::{show, AnimIn, Animator};
use crate::fx::{self, follow, FxList};
use crate::math::*;
use crate::moves::{mv, Hit, Move, BOLT_EVERY, BOLT_HIT, MOVES};
use crate::worldfile::Stage;

pub const DT: f32 = 1.0 / 60.0;

pub mod btn {
    pub const LIGHT: u32 = 1;
    pub const HEAVY: u32 = 2;
    pub const EVADE: u32 = 4;
    pub const UNSEAL: u32 = 8;
    pub const GUARD: u32 = 16;
    pub const HOVER: u32 = 32;
    pub const RESET: u32 = 64;
}

/// What happened in a tick, for sound and the interface.
pub mod ev {
    pub const SWING: u32 = 1 << 0;
    pub const HIT: u32 = 1 << 1;
    pub const KILL: u32 = 1 << 2;
    pub const HEAVY_HIT: u32 = 1 << 3;
    pub const CIRCLE: u32 = 1 << 4;
    pub const BEAM: u32 = 1 << 5;
    pub const LIGHTNING: u32 = 1 << 6;
    pub const FIRE: u32 = 1 << 7;
    pub const BOLT: u32 = 1 << 8;
    pub const BURST: u32 = 1 << 9;
    pub const EVADE: u32 = 1 << 10;
    pub const BLOCK: u32 = 1 << 11;
    pub const HURT: u32 = 1 << 12;
    pub const GATHER: u32 = 1 << 13;
    pub const UNSEAL: u32 = 1 << 14;
    pub const FALLEN: u32 = 1 << 15;
    pub const REVIVE: u32 = 1 << 16;
    pub const WON: u32 = 1 << 17;
    pub const READY: u32 = 1 << 18;
    pub const PILLAR: u32 = 1 << 19;
}

pub mod act {
    pub const FREE: u8 = 0;
    pub const MOVE: u8 = 1;
    pub const GUARD: u8 = 2;
    pub const HIT: u8 = 3;
    pub const DOWN: u8 = 4;
}

pub mod tune {
    pub const RUN_SPEED: f32 = 7.4;
    pub const HOVER_SPEED: f32 = 15.0;
    pub const HP_MAX: f32 = 1000.0;
    pub const MANA_MAX: f32 = 100.0;
    /// Ticks a pressed button waits for a move to accept it.
    pub const BUFFER: u32 = 16;
    /// Ticks without a hit before the chain count starts again.
    pub const CHAIN_HOLD: u32 = 150;
    pub const BODY: f32 = 0.4;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub buttons: u32,
    pub lx: f32,
    pub ly: f32,
    pub rx: f32,
    pub ry: f32,
}

pub struct Player {
    pub pos: V3,
    pub vel: V3,
    pub yaw: f32,
    pub act: u8,
    /// Ticks in the current action.
    pub t: u32,
    pub mv: u8,
    pub hp: f32,
    pub mana: f32,
    /// Strikes landed without a pause, and ticks since the last.
    pub chain: u32,
    pub chain_t: u32,
    /// Knights whose binding she has undone.
    pub kos: u32,
    pub hover: f32,
    pub phase: f32,
    pub spin: f32,
    buffer: u32,
    buffer_age: u32,
    prev: u32,
    hurt_t: u32,
    volley: u32,
}

/// A bolt of the volley in flight.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Bolt {
    pub pos: V3,
    pub vel: V3,
    pub target: V3,
    pub age: u32,
    pub alive: u32,
}

pub const BOLTS: usize = 16;

pub struct Camera {
    pub pos: V3,
    pub look: V3,
    pub fov: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub shake: f32,
    /// Narrowing of the view a heavy strike leaves behind, in degrees.
    pub punch: f32,
    idle: u32,
    focus: V3,
}

pub struct Sim {
    pub tick: u32,
    pub field: Field,
    pub stage: Stage,
    pub p: Player,
    pub cam: Camera,
    pub crowd: Crowd,
    pub fx: FxList,
    pub bolts: [Bolt; BOLTS],
    pub anim: Animator,
    pub events: u32,
    /// Ticks the current freeze still holds.
    pub stop: u32,
    /// The freeze just started, and how long it was asked to be.
    pub stop_len: u32,
    pub goal: u32,
    pub won: bool,
    pub auto: crate::auto::Auto,
    seed: u32,
}

impl Sim {
    pub fn new(field: Field, stage: Stage) -> Sim {
        let crowd = Crowd::new(&stage.musters);
        let goal = (crowd.n as u32 * 2 / 5).clamp(10, 1000);
        let mut s = Sim {
            tick: 0,
            p: Player { pos: V3::ZERO, vel: V3::ZERO, yaw: 0.0, act: act::FREE, t: 0, mv: 0, hp: tune::HP_MAX, mana: 0.0, chain: 0, chain_t: 0, kos: 0, hover: 0.0, phase: 0.0, spin: 0.0, buffer: 0, buffer_age: 0, prev: 0, hurt_t: 0, volley: 0 },
            cam: Camera { pos: V3::ZERO, look: v3(0.0, 0.0, -1.0), fov: 58.0, yaw: 0.0, pitch: 0.4, dist: 7.2, shake: 0.0, punch: 0.0, idle: 0, focus: V3::ZERO },
            crowd,
            fx: FxList::new(),
            bolts: [Bolt { pos: V3::ZERO, vel: V3::ZERO, target: V3::ZERO, age: 0, alive: 0 }; BOLTS],
            anim: Animator::new(),
            events: 0,
            stop: 0,
            stop_len: 0,
            goal,
            won: false,
            auto: crate::auto::Auto::new(),
            seed: 0x2545_f491,
            field,
            stage,
        };
        s.reset();
        s
    }

    pub fn reset(&mut self) {
        let (x, z, yaw) = self.stage.start;
        self.crowd = Crowd::new(&self.stage.musters);
        self.fx.clear();
        for b in &mut self.bolts {
            b.alive = 0;
        }
        self.tick = 0;
        self.stop = 0;
        self.won = false;
        self.events = 0;
        let p = &mut self.p;
        p.pos = self.field.point(x, z);
        p.vel = V3::ZERO;
        p.yaw = yaw;
        p.act = act::FREE;
        p.t = 0;
        p.mv = 0;
        p.hp = tune::HP_MAX;
        p.mana = 0.0;
        p.chain = 0;
        p.kos = 0;
        p.hover = 0.0;
        p.spin = 0.0;
        p.buffer = 0;
        p.volley = 0;
        self.cam.yaw = yaw;
        self.cam.pitch = 0.4;
        self.cam.shake = 0.0;
        self.cam.punch = 0.0;
        self.cam.focus = p.pos + v3(0.0, 1.15, 0.0);
        self.anim.reset();
        self.auto = crate::auto::Auto::new();
        self.update_camera(&Input::default(), true);
        self.pose(false);
    }

    fn rand(&mut self) -> u32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed
    }

    pub fn speed(&self) -> f32 {
        self.p.vel.flat().len()
    }

    pub fn current(&self) -> &'static Move {
        &MOVES[if self.p.act == act::MOVE { self.p.mv as usize } else { 0 }]
    }

    /// Whether a blow can land on her now.
    fn open(&self) -> bool {
        match self.p.act {
            act::DOWN => false,
            act::MOVE => {
                let m = &MOVES[self.p.mv as usize];
                !(self.p.t >= m.safe.0 as u32 && self.p.t < m.safe.1 as u32)
            }
            _ => true,
        }
    }

    fn start(&mut self, id: u8, stick: V3) {
        let p = &mut self.p;
        p.act = act::MOVE;
        p.mv = id;
        p.t = 0;
        p.buffer = 0;
        p.spin = 0.0;
        p.hover = 0.0;
        match id {
            mv::EVADE => {
                if stick.len2() > 0.09 {
                    p.yaw = yaw_of(stick);
                }
                self.events |= ev::EVADE;
            }
            mv::UNSEAL => {
                p.mana = 0.0;
                self.events |= ev::GATHER;
            }
            mv::L1..=mv::L5 => self.events |= ev::SWING,
            _ => {}
        }
    }

    /// A strike of hers lands: the crowd answers, the frame holds.
    fn land(&mut self, hit: &Hit, origin: V3, yaw: f32) -> Struck {
        let s = self.crowd.strike(hit, origin, yaw, self.tick, &mut self.fx);
        if s.count > 0 {
            if s.stop > self.stop {
                self.stop = s.stop;
                self.stop_len = s.stop;
            }
            let p = &mut self.p;
            p.chain += s.count;
            p.chain_t = 0;
            let was = p.mana;
            if hit.react != crate::moves::react::DISPEL {
                p.mana = min(p.mana + s.count as f32 * 0.45 + s.kills as f32 * 0.6, tune::MANA_MAX);
            }
            if was < tune::MANA_MAX && p.mana >= tune::MANA_MAX {
                self.events |= ev::READY;
            }
            p.kos += s.kills;
            self.cam.shake = max(self.cam.shake, hit.shake * (1.0 + min(s.count as f32, 10.0) * 0.06));
            self.cam.punch = max(self.cam.punch, hit.shake * 9.0);
            self.events |= ev::HIT | if hit.stop >= 5 { ev::HEAVY_HIT } else { 0 } | if s.kills > 0 { ev::KILL } else { 0 };
        }
        s
    }

    pub fn tick(&mut self, input: Input) {
        self.events = 0;
        if input.buttons & btn::RESET != 0 && self.p.prev & btn::RESET == 0 {
            self.reset();
            self.p.prev = input.buttons;
            return;
        }
        // ---- the buffer: the latest press waits for whatever will take it
        let pressed = input.buttons & !self.p.prev;
        self.p.prev = input.buttons;
        let want = [btn::UNSEAL, btn::EVADE, btn::HEAVY, btn::LIGHT].into_iter().find(|b| pressed & b != 0);
        if let Some(b) = want {
            self.p.buffer = b;
            self.p.buffer_age = 0;
        } else {
            self.p.buffer_age += 1;
            if self.p.buffer_age > tune::BUFFER {
                self.p.buffer = 0;
            }
        }

        let frozen = self.stop > 0;
        if frozen {
            // A strike holds her and what it struck; the rest of the field moves on.
            self.stop -= 1;
        } else {
            self.act(&input);
        }

        // ---- the army, and its blows on her
        let target = self.p.pos;
        self.crowd.tick(&self.field, self.tick, target, &mut self.fx);
        for k in 0..4 {
            let Some(b) = self.crowd.blows[k] else { continue };
            if !self.open() {
                continue;
            }
            let ahead = heading(self.p.yaw);
            if self.p.act == act::GUARD && b.dir.dot(ahead) < -0.2 {
                self.events |= ev::BLOCK;
                self.fx.spawn(self.tick, fx::kind::BLOCK, self.p.pos + ahead * 0.8 + v3(0.0, 1.1, 0.0), ahead, 1.0, 0);
                self.p.vel = b.dir * 2.5;
                self.p.mana = min(self.p.mana + 2.0, tune::MANA_MAX);
                self.cam.shake = max(self.cam.shake, 0.12);
                continue;
            }
            self.p.hp -= b.damage;
            self.p.hurt_t = 0;
            if self.p.act != act::MOVE {
                self.p.vel = b.dir * 4.0;
            }
            self.events |= ev::HURT;
            self.cam.shake = max(self.cam.shake, 0.45);
            self.fx.spawn(self.tick, fx::kind::HURT, self.p.pos + v3(0.0, 1.0, 0.0), b.dir, 1.0, 0);
            if self.p.hp <= 0.0 {
                self.p.hp = 0.0;
                self.p.act = act::DOWN;
                self.p.t = 0;
                self.events |= ev::FALLEN;
            } else if self.p.act != act::MOVE {
                // A blow staggers her when she stands or runs; in a move of her own she keeps her feet.
                self.p.act = act::HIT;
                self.p.t = 0;
                self.stop = self.stop.max(3);
                self.stop_len = self.stop;
            }
        }

        self.fly_bolts();

        if !self.won && self.p.kos >= self.goal {
            self.won = true;
            self.events |= ev::WON;
        }
        self.p.chain_t += 1;
        if self.p.chain_t > tune::CHAIN_HOLD {
            self.p.chain = 0;
        }
        self.p.hurt_t += 1;
        if self.p.hurt_t > 150 && self.p.act != act::DOWN {
            self.p.hp = min(self.p.hp + 60.0 * DT, tune::HP_MAX);
        }

        self.update_camera(&input, false);
        self.pose(frozen);
        // Effects that hold on to her follow her.
        for f in self.fx.items.iter_mut() {
            match f.follow {
                follow::STAFF => {
                    f.pos = self.anim.head;
                    f.dir = self.anim.staff;
                }
                follow::BODY => {
                    f.pos = self.p.pos;
                    f.dir = heading(self.p.yaw);
                }
                _ => {}
            }
        }
        self.tick = self.tick.wrapping_add(1);
    }

    /// The mage's own tick: what her state does with the stick and the buffer.
    fn act(&mut self, input: &Input) {
        let fwd = heading(self.cam.yaw);
        let right = v3(-fwd.z, 0.0, fwd.x);
        let mut stick = right * input.lx + fwd * input.ly;
        let mag = min(stick.len(), 1.0);
        if mag > 1e-3 {
            stick = stick * (mag / stick.len());
        }
        let buffer = self.p.buffer;
        match self.p.act {
            act::FREE | act::GUARD => {
                let guarding = input.buttons & btn::GUARD != 0;
                let hovering = input.buttons & btn::HOVER != 0 && mag > 0.15 && !guarding;
                self.p.hover = approach(self.p.hover, if hovering { 1.0 } else { 0.0 }, DT * 5.0);
                let top = if guarding { 2.2 } else { lerp(tune::RUN_SPEED, tune::HOVER_SPEED, self.p.hover) };
                let want = stick * top;
                let rate = if mag > 0.05 { 11.0 } else { 14.0 };
                self.p.vel = v3(ease(self.p.vel.x, want.x, rate, DT), 0.0, ease(self.p.vel.z, want.z, rate, DT));
                if mag > 0.1 && !guarding {
                    self.p.yaw = ease_angle(self.p.yaw, yaw_of(stick), 14.0, DT);
                } else if guarding {
                    // The barrier faces where the camera looks, or the nearest knight.
                    let to = self.crowd.nearest(self.p.pos, self.p.yaw, 6.0, 1.6).map(|(x, z)| yaw_of(v3(x - self.p.pos.x, 0.0, z - self.p.pos.z))).unwrap_or(self.p.yaw);
                    self.p.yaw = ease_angle(self.p.yaw, to, 8.0, DT);
                }
                if guarding && self.p.act == act::FREE {
                    self.fx.spawn(self.tick, fx::kind::GUARD, self.p.pos, heading(self.p.yaw), 1.0, follow::BODY);
                }
                if guarding && self.p.act == act::GUARD && self.p.t % 10 == 9 {
                    self.fx.spawn(self.tick, fx::kind::GUARD, self.p.pos, heading(self.p.yaw), 1.0, follow::BODY);
                }
                self.p.act = if guarding { act::GUARD } else { act::FREE };
                self.p.t += 1;
                match buffer {
                    btn::LIGHT => self.start(mv::L1, stick),
                    btn::HEAVY => self.start(mv::BEAM, stick),
                    btn::EVADE => self.start(mv::EVADE, stick),
                    btn::UNSEAL if self.p.mana >= tune::MANA_MAX => self.start(mv::UNSEAL, stick),
                    _ => {}
                }
            }
            act::MOVE => {
                let m = &MOVES[self.p.mv as usize];
                let t = self.p.t;
                if t < m.turn as u32 {
                    // She turns to the stick, or else to the knight nearest ahead.
                    let to = if mag > 0.3 { Some(yaw_of(stick)) } else { self.crowd.nearest(self.p.pos, self.p.yaw, 7.5, 1.3).map(|(x, z)| yaw_of(v3(x - self.p.pos.x, 0.0, z - self.p.pos.z))) };
                    if let Some(to) = to {
                        self.p.yaw = ease_angle(self.p.yaw, to, 16.0, DT);
                    }
                }
                let ahead = heading(self.p.yaw);
                if t >= m.lunge.0 as u32 && t < m.lunge.1 as u32 {
                    self.p.vel = ahead * m.lunge.2;
                } else {
                    let k = exp(-14.0 * DT);
                    self.p.vel = self.p.vel * k;
                }
                self.p.spin = if t >= m.spin.0 as u32 && t < m.spin.1 as u32 {
                    let u = (t - m.spin.0 as u32) as f32 / (m.spin.1 - m.spin.0) as f32;
                    -TAU * u * u * (3.0 - 2.0 * u)
                } else {
                    0.0
                };
                for c in m.cues {
                    if c.at as u32 != t {
                        continue;
                    }
                    let (pos, dir) = (self.p.pos, ahead);
                    match c.fx {
                        fx::kind::VOLLEY => self.p.volley = c.a as u32,
                        fx::kind::CIRCLE => {
                            self.fx.spawn(self.tick, c.fx, self.anim.head, dir, c.a, follow::STAFF);
                            self.events |= ev::CIRCLE;
                        }
                        fx::kind::GATHER => self.fx.spawn(self.tick, c.fx, pos, dir, c.a, follow::BODY),
                        fx::kind::BEAM => {
                            self.fx.spawn(self.tick, c.fx, self.anim.head, dir, c.a, 0);
                            self.events |= ev::BEAM;
                            self.cam.shake = max(self.cam.shake, 0.3);
                        }
                        fx::kind::CONE => {
                            self.fx.spawn(self.tick, c.fx, self.anim.head, dir, c.a, 0);
                            self.events |= ev::BURST;
                        }
                        fx::kind::LIGHTNING => {
                            self.fx.spawn(self.tick, c.fx, self.anim.head, dir, c.a, 0);
                            self.events |= ev::LIGHTNING;
                            self.cam.shake = max(self.cam.shake, 0.25);
                        }
                        fx::kind::HELLFIRE => {
                            let at = self.field.point(pos.x + dir.x * 7.0, pos.z + dir.z * 7.0);
                            self.fx.spawn(self.tick, c.fx, at, dir, c.a, 0);
                            self.events |= ev::FIRE;
                        }
                        fx::kind::UNSEAL => {
                            self.fx.spawn(self.tick, c.fx, pos, dir, c.a, 0);
                            self.events |= ev::UNSEAL;
                            self.cam.shake = 1.0;
                        }
                        fx::kind::PILLAR => {
                            let at = self.field.point(pos.x + dir.x * 1.5, pos.z + dir.z * 1.5);
                            self.fx.spawn(self.tick, c.fx, at, dir, c.a, 0);
                            self.events |= ev::PILLAR;
                        }
                        fx::kind::SHOCK => {
                            let at = self.field.point(pos.x + dir.x * 1.7, pos.z + dir.z * 1.7);
                            self.fx.spawn(self.tick, c.fx, at, dir, c.a, 0);
                        }
                        k => self.fx.spawn(self.tick, k, pos, dir, c.a, 0),
                    }
                }
                for h in m.hits {
                    if h.at as u32 == t {
                        self.land(h, self.p.pos, self.p.yaw);
                    }
                }
                // The volley: a bolt every few ticks while she holds the pose.
                if self.p.volley > 0 && t % BOLT_EVERY == 0 {
                    self.fire_bolt();
                }
                self.p.t += 1;
                let t = self.p.t;
                if t >= m.cancel as u32 && buffer != 0 {
                    let next = match buffer {
                        btn::LIGHT => m.next_light,
                        btn::HEAVY => m.next_heavy,
                        btn::EVADE if self.p.mv != mv::EVADE => mv::EVADE,
                        btn::UNSEAL if self.p.mana >= tune::MANA_MAX => mv::UNSEAL,
                        _ => mv::NONE,
                    };
                    if next != mv::NONE {
                        self.start(next, stick);
                        return self.step();
                    }
                }
                if t >= m.len as u32 || (t >= m.cancel as u32 + 8 && mag > 0.5 && self.p.volley == 0) {
                    self.p.act = act::FREE;
                    self.p.t = 0;
                    self.p.spin = 0.0;
                }
            }
            act::HIT => {
                self.p.vel = self.p.vel * exp(-9.0 * DT);
                self.p.t += 1;
                if self.p.t >= 14 {
                    self.p.act = act::FREE;
                    self.p.t = 0;
                }
            }
            _ => {
                self.p.vel = self.p.vel * exp(-9.0 * DT);
                self.p.t += 1;
                if self.p.t == 150 {
                    // She gets up, and what stood over her is thrown back.
                    self.p.hp = tune::HP_MAX * 0.6;
                    self.events |= ev::REVIVE;
                    let wave = Hit { at: 0, shape: crate::moves::Shape::Ring { r: 9.0 }, damage: 20, push: 12.0, lift: 6.0, stop: 6, react: crate::moves::react::LAUNCH, fx: fx::kind::SPARK, shake: 0.5 };
                    self.fx.spawn(self.tick, fx::kind::SHOCK, self.p.pos, V3::UP, 9.0, 0);
                    self.land(&wave, self.p.pos, self.p.yaw);
                    self.p.act = act::FREE;
                    self.p.t = 0;
                }
            }
        }
        self.step();
    }

    /// Moves her by her velocity over the ground.
    fn step(&mut self) {
        let p = &mut self.p;
        let (mut x, mut z) = (p.pos.x + p.vel.x * DT, p.pos.z + p.vel.z * DT);
        self.field.push_out(&mut x, &mut z, tune::BODY);
        p.pos = self.field.point(x, z);
        let speed = p.vel.flat().len();
        if p.act == act::FREE || p.act == act::GUARD {
            p.phase += speed * DT / crate::anim::stride(speed, 0.71) * TAU;
            if p.phase > TAU {
                p.phase -= TAU;
            }
        }
    }

    fn fire_bolt(&mut self) {
        let Some(slot) = self.bolts.iter().position(|b| b.alive == 0) else { return };
        self.p.volley -= 1;
        let k = self.p.volley;
        let ahead = heading(self.p.yaw);
        let right = v3(-ahead.z, 0.0, ahead.x);
        // The circles stand in an arc behind and above her.
        let u = (k % 5) as f32 - 2.0;
        let from = self.p.pos + v3(0.0, 2.5 - 0.14 * u * u, 0.0) + right * (u * 0.85) - ahead * 0.7;
        let spread = ((self.rand() & 255) as f32 / 255.0 - 0.5) * 1.3;
        let probe = self.p.pos + heading(self.p.yaw + spread) * (6.0 + (k % 4) as f32 * 4.5);
        let target = match self.crowd.nearest(probe, self.p.yaw, 9.0, PI) {
            Some((x, z)) => self.field.point(x, z) + v3(0.0, 0.9, 0.0),
            None => self.field.point(probe.x, probe.z) + v3(0.0, 0.4, 0.0),
        };
        self.bolts[slot] = Bolt { pos: from, vel: (ahead + v3(0.0, 0.55, 0.0) + right * (u * 0.22)).norm() * 30.0, target, age: 0, alive: 1 };
        self.fx.spawn(self.tick, fx::kind::MUZZLE, from, ahead, 1.0, 0);
        self.events |= ev::BOLT;
    }

    fn fly_bolts(&mut self) {
        for i in 0..BOLTS {
            let mut b = self.bolts[i];
            if b.alive == 0 {
                continue;
            }
            b.age += 1;
            let to = b.target - b.pos;
            let d = to.len();
            // It bends toward its target harder the longer it has flown.
            let steer = min(0.06 + b.age as f32 * 0.012, 0.5);
            b.vel = (b.vel.norm_or(V3::UP) * (1.0 - steer) + to.norm_or(V3::UP) * steer).norm_or(V3::UP) * 42.0;
            b.pos += b.vel * DT;
            let ground = self.field.height(b.pos.x, b.pos.z);
            if d < 1.4 || b.pos.y < ground + 0.1 || b.age > 110 {
                b.alive = 0;
                let at = v3(b.pos.x, max(b.pos.y, ground + 0.3), b.pos.z);
                self.fx.spawn(self.tick, fx::kind::BURST, at, V3::UP, 3.2, 0);
                self.land(&BOLT_HIT, at, yaw_of(b.vel));
                self.events |= ev::BURST;
            }
            self.bolts[i] = b;
        }
    }

    fn update_camera(&mut self, input: &Input, snap: bool) {
        let c = &mut self.cam;
        let p = &self.p;
        let turning = abs(input.rx) > 0.08 || abs(input.ry) > 0.08;
        c.yaw = wrap_angle(c.yaw - input.rx * 2.9 * DT);
        c.pitch = clamp(c.pitch - input.ry * 1.7 * DT, -0.18, 1.0);
        c.idle = if turning { 0 } else { c.idle + 1 };
        let moving = p.vel.flat().len();
        if c.idle > 40 && moving > 2.0 && p.act == act::FREE {
            // Left alone, it comes round behind the way she runs.
            let want = yaw_of(p.vel);
            let d = wrap_angle(want - c.yaw);
            // Running toward the camera does not swing it.
            if abs(d) < 2.5 {
                c.yaw = wrap_angle(c.yaw + d * (1.0 - exp(-(0.9 + p.hover * 1.2) * DT)));
            }
            c.pitch = ease(c.pitch, 0.4, 0.6, DT);
        }
        if c.idle > 120 {
            c.pitch = ease(c.pitch, 0.4, 0.8, DT);
        }
        let unsealing = p.act == act::MOVE && p.mv == mv::UNSEAL;
        let dist = if unsealing { 10.5 } else { 7.2 + p.hover * 1.6 };
        c.dist = if snap { dist } else { ease(c.dist, dist, 3.0, DT) };
        let focus = p.pos + v3(0.0, 1.35 + p.hover * 0.3, 0.0) + p.vel * 0.1;
        c.focus = if snap { focus } else { c.focus.ease(focus, 12.0, DT) };
        let dir = forward(c.yaw, -c.pitch);
        let mut pos = c.focus - dir * c.dist;
        let floor = self.field.height(pos.x, pos.z) + 0.45;
        if pos.y < floor {
            pos.y = floor;
        }
        c.pos = pos;
        c.look = (c.focus - pos).norm_or(dir);
        c.shake *= exp(-7.0 * DT);
        c.punch *= exp(-6.0 * DT);
        c.fov = 58.0 + p.hover * 7.0 - c.punch + if unsealing { 6.0 } else { 0.0 };
    }

    fn pose(&mut self, frozen: bool) {
        let p = &self.p;
        let show = match p.act {
            act::MOVE => show::MOVE,
            act::GUARD => show::GUARD,
            act::HIT => show::HIT,
            act::DOWN => show::DOWN,
            _ => show::FREE,
        };
        // Held in a freeze, she shivers in place.
        let mut pos = p.pos;
        if frozen && self.stop_len >= 4 {
            let k = 0.02 * self.stop as f32 / self.stop_len as f32;
            pos = pos + v3(sin(self.tick as f32 * 2.7) * k, 0.0, cos(self.tick as f32 * 3.1) * k);
        }
        self.anim.update(&AnimIn { pos, yaw: p.yaw + p.spin, speed: p.vel.flat().len(), show, mv: p.mv, t: p.t as f32, phase: p.phase, hover: p.hover, tick: self.tick, frozen });
    }

    /// The autopilot's input for the next tick.
    pub fn auto_input(&mut self) -> Input {
        let mut auto = core::mem::replace(&mut self.auto, crate::auto::Auto::new());
        let i = auto.input(self);
        self.auto = auto;
        i
    }
}
