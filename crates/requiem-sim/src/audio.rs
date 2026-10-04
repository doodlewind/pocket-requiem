//! Sound, synthesized: night wind, the tread of the army, and a short voice
//! for each event (a staff's sweep, plate struck, a beam, lightning, fire,
//! the bell of an undone binding). There are no samples; the reference and
//! the devices render the same code.
//!
//! The synthesizer listens to the simulation and never feeds back into it.

use crate::math::*;
use crate::sim::{ev, Sim};

const VOICES: usize = 14;

mod kind {
    pub const SWISH: u8 = 1;
    pub const CLANG: u8 = 2;
    pub const THUD: u8 = 3;
    pub const CHIME: u8 = 4;
    pub const BEAM: u8 = 5;
    pub const CRACK: u8 = 6;
    pub const ROAR: u8 = 7;
    pub const POP: u8 = 8;
    pub const WHIP: u8 = 9;
    pub const PING: u8 = 10;
    pub const GRUNT: u8 = 11;
    pub const SWELL: u8 = 12;
    pub const BELL: u8 = 13;
    pub const ZAP: u8 = 14;
}

#[derive(Clone, Copy)]
struct Voice {
    kind: u8,
    t: f32,
    gain: f32,
    lp: f32,
    phase: f32,
}

pub struct Synth {
    seed: u32,
    voices: [Voice; VOICES],
    // Continuous layers: current value and target.
    wind: [f32; 2],
    tread: [f32; 2],
    hum: [f32; 2],
    wind_lp: [f32; 2],
    tread_lp: f32,
    tread_phase: f32,
    hum_phase: f32,
    gust: f32,
}

impl Synth {
    pub fn new() -> Synth {
        Synth { seed: 0x1234_5678, voices: [Voice { kind: 0, t: 0.0, gain: 0.0, lp: 0.0, phase: 0.0 }; VOICES], wind: [0.12; 2], tread: [0.0; 2], hum: [0.0; 2], wind_lp: [0.0; 2], tread_lp: 0.0, tread_phase: 0.0, hum_phase: 0.0, gust: 0.0 }
    }

    #[inline]
    fn noise(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed >> 8) as f32 / 8388608.0 - 1.0
    }

    fn play(&mut self, kind: u8, gain: f32) {
        // The oldest voice gives way when all are busy.
        let slot = self.voices.iter().position(|v| v.kind == 0).unwrap_or_else(|| self.voices.iter().enumerate().max_by(|a, b| a.1.t.total_cmp(&b.1.t)).map(|v| v.0).unwrap_or(0));
        self.voices[slot] = Voice { kind, t: 0.0, gain, lp: 0.0, phase: 0.0 };
    }

    /// Takes the simulation's state after a tick, and that tick's events.
    pub fn control(&mut self, sim: &Sim, events: u32) {
        self.wind[1] = 0.1 + 0.25 * sim.p.hover + 0.02 * sin(sim.tick as f32 * 0.011);
        self.gust = 0.02 + 0.05 * sim.p.hover;
        self.tread[1] = min(sim.crowd.near as f32 / 60.0, 1.0) * 0.22;
        let casting = sim.p.act == crate::sim::act::MOVE && sim.p.mv == crate::moves::mv::UNSEAL && sim.p.t < 78;
        self.hum[1] = if casting { 0.1 + 0.25 * sim.p.t as f32 / 78.0 } else { 0.0 };
        for (bit, kind, gain) in [
            (ev::SWING, kind::SWISH, 0.8),
            (ev::HIT, kind::CLANG, 0.7),
            (ev::HEAVY_HIT, kind::THUD, 1.0),
            (ev::KILL, kind::BELL, 0.5),
            (ev::CIRCLE, kind::CHIME, 0.6),
            (ev::BEAM, kind::BEAM, 1.0),
            (ev::LIGHTNING, kind::CRACK, 1.0),
            (ev::FIRE, kind::ROAR, 1.0),
            (ev::PILLAR, kind::SWELL, 0.7),
            (ev::BOLT, kind::ZAP, 0.5),
            (ev::BURST, kind::POP, 0.8),
            (ev::EVADE, kind::WHIP, 0.8),
            (ev::BLOCK, kind::PING, 1.0),
            (ev::HURT, kind::GRUNT, 1.0),
            (ev::GATHER, kind::SWELL, 1.0),
            (ev::UNSEAL, kind::ROAR, 1.2),
            (ev::UNSEAL | ev::WON | ev::READY | ev::REVIVE, kind::BELL, 1.0),
        ] {
            if events & bit != 0 {
                self.play(kind, gain);
            }
        }
    }

    /// Renders interleaved stereo at `rate` Hz into `out` (two values per frame).
    pub fn render(&mut self, out: &mut [i16], rate: f32) {
        let dt = 1.0 / rate;
        let glide = 1.0 - exp(-dt / 0.05);
        let scale = (44100.0 / rate).min(2.0);
        for frame in out.chunks_exact_mut(2) {
            for layer in [&mut self.wind, &mut self.tread, &mut self.hum] {
                layer[0] += (layer[1] - layer[0]) * glide;
            }
            // Wind: noise through a low-pass that breathes; a little width between the ears.
            let (n0, n1) = (self.noise(), self.noise());
            let cut = (0.012 + self.gust) * scale;
            self.wind_lp[0] += (n0 - self.wind_lp[0]) * cut;
            self.wind_lp[1] += (n0 * 0.6 + n1 * 0.4 - self.wind_lp[1]) * cut;
            let wind_l = self.wind_lp[0] * self.wind[0] * 3.2;
            let wind_r = self.wind_lp[1] * self.wind[0] * 3.2;
            // The army: low noise gated at marching pace, with a rattle of plate on top.
            self.tread_phase += dt * 3.4;
            if self.tread_phase > 1.0 {
                self.tread_phase -= 1.0;
            }
            let gate = exp(-self.tread_phase * 7.0);
            let n2 = self.noise();
            self.tread_lp += (n2 - self.tread_lp) * 0.02 * scale;
            let mut mono = (self.tread_lp * 5.0 * (0.4 + gate) + (n2 - self.tread_lp) * 0.06 * gate) * self.tread[0];
            // Gathering mana: a rising hum.
            self.hum_phase += TAU * (90.0 + 420.0 * self.hum[0]) * dt;
            if self.hum_phase > TAU {
                self.hum_phase -= TAU;
            }
            mono += (sin(self.hum_phase) + 0.4 * sin(self.hum_phase * 2.01) + 0.2 * sin(self.hum_phase * 3.02)) * self.hum[0] * 0.5;

            for i in 0..VOICES {
                let v = self.voices[i];
                if v.kind == 0 {
                    continue;
                }
                let t = v.t;
                let n = self.noise();
                let mut lp = v.lp;
                let mut phase = v.phase;
                let (s, dur) = match v.kind {
                    kind::SWISH => {
                        lp += (n - lp) * (0.06 + 2.2 * t) * scale;
                        ((n - lp) * 0.38 * sin(PI * min(t / 0.2, 1.0)), 0.2)
                    }
                    kind::CLANG => ((sin(TAU * 1730.0 * t) + 0.7 * sin(TAU * 2840.0 * t) + 0.5 * sin(TAU * 4310.0 * t)) * 0.14 * exp(-t * 26.0) + n * 0.3 * exp(-t * 220.0), 0.2),
                    kind::THUD => {
                        phase += TAU * 110.0 * exp(-t * 7.0) * dt;
                        lp += (n - lp) * 0.1 * scale;
                        (sin(phase) * 0.9 * exp(-t * 11.0) + lp * 0.8 * exp(-t * 30.0), 0.34)
                    }
                    kind::CHIME => ((sin(TAU * 1320.0 * t) + 0.5 * sin(TAU * 1980.0 * t)) * 0.16 * exp(-t * 7.0), 0.5),
                    kind::BEAM => {
                        phase += TAU * (240.0 + 900.0 * exp(-t * 5.0)) * dt;
                        lp += (n - lp) * 0.25 * scale;
                        ((sin(phase) * 0.35 + sin(phase * 0.5) * 0.3 + (n - lp) * 0.3) * min(t * 40.0, 1.0) * exp(-t * 4.5), 0.6)
                    }
                    kind::CRACK => {
                        lp += (n - lp) * 0.5 * scale;
                        (n * 0.7 * exp(-t * 55.0) + lp * 0.9 * exp(-t * 7.0) * (0.6 + 0.4 * sin(TAU * 31.0 * t)), 0.5)
                    }
                    kind::ROAR => {
                        lp += (n - lp) * 0.035 * scale;
                        phase += TAU * 52.0 * dt;
                        (lp * 4.5 * min(t * 14.0, 1.0) * exp(-t * 2.6) + sin(phase) * 0.5 * exp(-t * 5.0), 1.1)
                    }
                    kind::POP => {
                        phase += TAU * 180.0 * exp(-t * 14.0) * dt;
                        lp += (n - lp) * 0.2 * scale;
                        (sin(phase) * 0.6 * exp(-t * 16.0) + lp * 0.7 * exp(-t * 20.0), 0.25)
                    }
                    kind::WHIP => {
                        lp += (n - lp) * (0.4 - 1.5 * t).max(0.03) * scale;
                        (lp * 0.9 * sin(PI * min(t / 0.18, 1.0)), 0.18)
                    }
                    kind::PING => ((sin(TAU * 2300.0 * t) + 0.6 * sin(TAU * 3450.0 * t)) * 0.2 * exp(-t * 16.0), 0.3),
                    kind::GRUNT => {
                        phase += TAU * 150.0 * exp(-t * 4.0) * dt;
                        lp += (n - lp) * 0.08 * scale;
                        (sin(phase) * 0.5 * exp(-t * 14.0) + lp * 1.2 * exp(-t * 18.0), 0.25)
                    }
                    kind::SWELL => {
                        phase += TAU * (160.0 + 500.0 * t) * dt;
                        (sin(phase) * 0.22 * sin(PI * min(t / 0.45, 1.0)), 0.45)
                    }
                    kind::BELL => ((sin(TAU * 880.0 * t) + 0.6 * sin(TAU * 1318.0 * t) + 0.35 * sin(TAU * 2637.0 * t)) * 0.16 * exp(-t * 3.2), 1.2),
                    _ => {
                        phase += TAU * (1900.0 - 1400.0 * min(t * 8.0, 1.0)) * dt;
                        (sin(phase) * 0.25 * exp(-t * 20.0), 0.14)
                    }
                };
                mono += s * v.gain;
                let nt = t + dt;
                self.voices[i] = if nt >= dur { Voice { kind: 0, ..v } } else { Voice { t: nt, lp, phase, ..v } };
            }
            // Soft limit, then 16 bits.
            let clip = |x: f32| (x / (1.0 + abs(x)) * 30000.0) as i16;
            frame[0] = clip(mono + wind_l);
            frame[1] = clip(mono + wind_r);
        }
    }
}

impl Default for Synth {
    fn default() -> Self {
        Self::new()
    }
}
