//! The autopilot: it plays through the same inputs as a person. It walks into
//! the nearest part of the army, works through the chains in turn, unseals
//! when the gauge is full and the press is thick, and hovers to the next
//! cohort when the ground around it is clear. It is the attract mode and the
//! repeatable load for measurements.

use crate::math::*;
use crate::sim::{act, btn, tune, Input, Sim};

/// The chains it cycles through: light strikes, then whether a heavy one ends them.
const CHAINS: [(u32, bool); 8] = [(3, true), (4, true), (1, true), (5, false), (2, true), (0, true), (4, true), (3, true)];

pub struct Auto {
    chain: usize,
    /// Presses made in the current chain.
    step: u32,
    /// Ticks until the next press.
    wait: u32,
    held: bool,
    pub chains: u32,
}

impl Auto {
    pub fn new() -> Auto {
        Auto { chain: 0, step: 0, wait: 0, held: false, chains: 0 }
    }

    pub fn input(&mut self, s: &Sim) -> Input {
        let p = &s.p;
        let mut out = Input::default();
        if p.act == act::DOWN {
            return out;
        }
        let fwd = heading(s.cam.yaw);
        let right = v3(-fwd.z, 0.0, fwd.x);
        let steer = |to: V3, out: &mut Input| {
            let d = (to - p.pos).flat().norm_or(fwd);
            out.lx = d.dot(right);
            out.ly = d.dot(fwd);
        };
        let close = s.crowd.count_near(p.pos, 3.4);
        let around = s.crowd.count_near(p.pos, 9.0);
        let nearest = s.crowd.nearest(p.pos, p.yaw, 70.0, PI);

        // A press is one tick down, then at least one tick up.
        if self.held {
            self.held = false;
            return out;
        }
        if p.mana >= tune::MANA_MAX && around >= 10 && (p.act == act::FREE || s.current().cancel as u32 <= p.t) {
            out.buttons = btn::UNSEAL;
            self.held = true;
            return out;
        }
        let fighting = close >= 1 || (p.act == act::MOVE && around >= 1);
        if fighting {
            if let Some((x, z)) = nearest {
                steer(v3(x, 0.0, z), &mut out);
                // A nudge of the stick, so a strike turns to them without walking her out of the chain.
                out.lx *= 0.4;
                out.ly *= 0.4;
            }
            if self.wait > 0 {
                self.wait -= 1;
                return out;
            }
            let (lights, heavy) = CHAINS[self.chain];
            let ready = p.act == act::FREE || p.t + 3 >= s.current().cancel as u32;
            if ready {
                if self.step < lights {
                    out.buttons = btn::LIGHT;
                    self.step += 1;
                } else if heavy && self.step == lights {
                    out.buttons = btn::HEAVY;
                    self.step += 1;
                } else {
                    self.step = 0;
                    self.chain = (self.chain + 1) % CHAINS.len();
                    self.chains += 1;
                    self.wait = 14;
                    return out;
                }
                self.held = true;
                self.wait = 5;
            }
            return out;
        }
        self.step = 0;
        // Nothing in reach: go to the nearest knight, or the nearest cohort still in its ranks.
        let to = nearest.or_else(|| s.crowd.nearest_host(p.pos));
        if let Some((x, z)) = to {
            let goal = v3(x, 0.0, z);
            steer(goal, &mut out);
            if (goal - p.pos).flat().len() > 22.0 {
                out.buttons |= btn::HOVER;
            }
        }
        out
    }
}

impl Default for Auto {
    fn default() -> Self {
        Self::new()
    }
}
