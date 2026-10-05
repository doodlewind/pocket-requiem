//! The loop around the simulation that every handheld shares: pad to input,
//! the autopilot, notes on the screen, the camera, the frame-rate governor,
//! the interface's contents, remote settings and the status record.

use alloc::format;
use alloc::string::String;
use core::fmt::Write;

use requiem_sim::audio::Synth;
use requiem_sim::math::*;
use requiem_sim::sim::{act, ev, tune, Input};
use requiem_sim::Sim;

use crate::hud::{rgba, Hud};
use crate::mat::{self, Mat4};
use crate::scene::Scene;

/// Pad buttons beyond the simulation's (`requiem_sim::sim::btn`, the low bits).
pub mod pad {
    pub const START: u32 = 1 << 16;
    pub const SELECT: u32 = 1 << 17;
    /// The simulation's buttons.
    pub const PLAY: u32 = 0xffff;
}

/// Display refreshes a frame is given: 30 frames a second on a 60 Hz screen.
pub const PACE: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Pad {
    pub buttons: u32,
    /// The stick, -1..1; `ly` positive is forward.
    pub lx: f32,
    pub ly: f32,
    /// The camera, -1..1: a second stick, or the direction pad on a machine with one stick.
    pub rx: f32,
    pub ry: f32,
}

pub struct Settings {
    pub auto: bool,
    pub hud: bool,
    pub stats: bool,
    pub world: bool,
    pub crowd: bool,
    pub mage: bool,
    pub fx: bool,
    pub lod_near: f32,
    pub lod_mid: f32,
    pub lod_far: f32,
    /// Draws the world this many times, to find how much time is left.
    pub repeat: u32,
    /// A fixed camera: eye, target, vertical field of view.
    pub view: Option<(V3, V3, f32)>,
    /// A device-specific switch a remote sets, for experiments.
    pub option: i32,
    /// Pulls distances in while frames are late.
    pub govern: bool,
    /// Display refreshes per frame.
    pub pace: u32,
}

#[derive(Clone, Copy)]
pub struct Camera {
    pub eye: V3,
    pub look: V3,
    pub fov: f32,
    pub roll: f32,
}

/// Rolling frame statistics over the last `N` frames.
pub struct Timing {
    ms: [f32; Timing::N],
    at: usize,
    pub late: u32,
    pub frames: u32,
}

impl Timing {
    const N: usize = 120;
    pub fn new() -> Timing {
        Timing { ms: [33.3; Timing::N], at: 0, late: 0, frames: 0 }
    }
    /// `late`: the frame took more display refreshes than its pace.
    pub fn push(&mut self, ms: f32, late: bool) {
        self.ms[self.at] = ms;
        self.at = (self.at + 1) % Self::N;
        self.frames += 1;
        if late {
            self.late += 1;
        }
    }
    pub fn avg(&self) -> f32 {
        self.ms.iter().sum::<f32>() / Self::N as f32
    }
    pub fn worst(&self) -> f32 {
        self.ms.iter().fold(0.0, |a, &b| max(a, b))
    }
}

impl Default for Timing {
    fn default() -> Self {
        Self::new()
    }
}

/// What a device measured, for the statistics line and the status record. Times are milliseconds.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Perf {
    pub frame: f32,
    pub worst: f32,
    pub late: u32,
    pub frames: u32,
    pub sim: f32,
    pub build: f32,
    pub draw: f32,
    pub gpu: f32,
    pub draws: u32,
    pub tris: u32,
}

/// What the frame's lists held, for the statistics line and the status record.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Info {
    pub crowd: crate::crowd::Stats,
    pub fx: crate::fx::Batches,
    pub patches: u32,
    pub built: u32,
    pub props: u32,
}

pub struct Game {
    pub sim: Sim,
    pub synth: Synth,
    pub scene: Scene,
    pub set: Settings,
    pub info: Info,
    note: (String, f32),
    prev_buttons: u32,
    pub frame: u32,
    /// The governor's factor on the army's distances and the world's middle and far ones, `FLOOR..=1`.
    pub lod_scale: f32,
    /// Frames and late frames in the governor's current window, and windows in a row without a late frame.
    window: (u32, u32, u32),
    /// The statistics line, rebuilt a few times a second.
    stats_line: String,
    /// The controls, as this machine labels them.
    help: &'static str,
}

impl Game {
    pub fn new(mut sim: Sim, scene: Scene, help: &'static str) -> Game {
        if scene.h.crowd_free >= 1.0 {
            sim.crowd.free_cap = scene.h.crowd_free as usize;
        }
        let set = Settings { auto: true, hud: true, stats: false, world: true, crowd: true, mage: true, fx: true, lod_near: scene.h.lod_near, lod_mid: scene.h.lod_mid, lod_far: scene.h.lod_far, repeat: 1, view: None, option: 0, govern: true, pace: PACE };
        Game { sim, synth: Synth::new(), scene, set, info: Info::default(), note: (String::new(), 0.0), prev_buttons: u32::MAX, frame: 0, lod_scale: 1.0, window: (0, 0, 0), stats_line: String::new(), help }
    }

    fn say(&mut self, text: &str, seconds: f32) {
        self.note.0.clear();
        self.note.0.push_str(text);
        self.note.1 = seconds;
    }

    /// One displayed frame: `ticks` simulation ticks (one per display refresh since the last frame).
    pub fn step(&mut self, pad: &Pad, ticks: u32) -> u32 {
        let pressed = pad.buttons & !self.prev_buttons;
        self.prev_buttons = pad.buttons;
        if pressed & pad::SELECT != 0 && self.frame > 30 {
            self.sim.reset();
            self.set.auto = false;
            self.say("AGAIN", 1.2);
        }
        if pressed & pad::START != 0 {
            self.set.auto = !self.set.auto;
            let text = if self.set.auto { "AUTOPILOT" } else { "MANUAL" };
            self.say(text, 1.5);
        }
        // Any deliberate input takes over from the autopilot.
        if self.set.auto && pressed & pad::PLAY != 0 && self.frame > 30 {
            self.set.auto = false;
            self.say("MANUAL", 1.5);
        }
        let mut events = 0u32;
        for _ in 0..ticks {
            let inp = if self.set.auto { self.sim.auto_input() } else { Input { buttons: pad.buttons & pad::PLAY, lx: pad.lx, ly: pad.ly, rx: pad.rx, ry: pad.ry } };
            self.sim.tick(inp);
            events |= self.sim.events;
            self.synth.control(&self.sim, self.sim.events);
        }
        if events & ev::READY != 0 {
            self.say("MANA FULL", 1.4);
        }
        if events & ev::UNSEAL != 0 {
            self.say("THE BINDING COMES UNDONE", 2.2);
        }
        if events & ev::FALLEN != 0 {
            self.say("FALLEN", 2.5);
        }
        if events & ev::WON != 0 {
            self.say("EVERY BINDING IS UNDONE.", 6.0);
        }
        self.note.1 -= ticks as f32 / 60.0;
        self.frame = self.frame.wrapping_add(1);
        self.govern(ticks > self.set.pace);
        events
    }

    /// The frame-rate governor. Over each second of frames: more than one late
    /// frame in ten pulls the army's distances and the world's middle and far
    /// ones in by 8 %; three seconds in a row without a late frame let them out
    /// by 4 %. What is beside the mage does not change.
    fn govern(&mut self, late: bool) {
        const WINDOW: u32 = 30;
        const FLOOR: f32 = 0.5;
        if !self.set.govern {
            self.lod_scale = 1.0;
            self.window = (0, 0, 0);
            return;
        }
        self.window.0 += 1;
        if late {
            self.window.1 += 1;
        }
        if self.window.0 < WINDOW {
            return;
        }
        if self.window.1 * 10 > WINDOW {
            self.lod_scale = max(self.lod_scale * 0.92, FLOOR);
            self.window.2 = 0;
        } else if self.window.1 == 0 {
            self.window.2 += 1;
            if self.window.2 >= 3 {
                self.lod_scale = min(self.lod_scale * 1.04, 1.0);
                self.window.2 = 0;
            }
        } else {
            self.window.2 = 0;
        }
        self.window.0 = 0;
        self.window.1 = 0;
    }

    /// Hands the device's measurements in for the statistics line, which is rebuilt every 10 frames.
    pub fn measure(&mut self, perf: &Perf) {
        if !self.set.stats || self.frame % 10 != 0 {
            return;
        }
        self.stats_line.clear();
        let _ = write!(
            self.stats_line,
            "{} fps {} ms late {}  cpu {}+{}  gpu {}  {} draws {}k tris  {} knights",
            F1(1000.0 / max(perf.frame, 0.1)),
            F1(perf.frame),
            perf.late,
            F1(perf.sim),
            F1(perf.build),
            F1(perf.gpu),
            perf.draws,
            F1(perf.tris as f32 / 1000.0),
            self.info.crowd.shown + self.info.crowd.far
        );
    }

    /// This frame's near, middle and far distances.
    pub fn lod(&self) -> (f32, f32, f32) {
        (self.set.lod_near, max(self.set.lod_mid * self.lod_scale, self.set.lod_near), self.set.lod_far * self.lod_scale)
    }

    pub fn camera(&self) -> Camera {
        match self.set.view {
            Some((pos, target, fov)) => Camera { eye: pos, look: (target - pos).norm_or(v3(0.0, 0.0, -1.0)), fov, roll: 0.0 },
            None => {
                let c = &self.sim.cam;
                let k = c.shake * 0.22;
                let t = self.sim.tick as f32;
                Camera { eye: c.pos + v3(sin(t * 1.7) * k, sin(t * 2.3) * k, cos(t * 1.9) * k), look: c.look, fov: c.fov, roll: 0.0 }
            }
        }
    }

    pub fn aspect(&self) -> f32 {
        self.scene.h.screen[0] / self.scene.h.screen[1]
    }

    /// Projection × view for culling and for placing interface marks.
    pub fn view_proj(&self, cam: &Camera) -> Mat4 {
        mat::mul(&mat::perspective(cam.fov, self.aspect(), self.scene.h.clip_near, self.scene.h.clip_far), &mat::view(cam.eye, cam.look, cam.roll))
    }

    /// How much of a heavy strike's freeze is left, 1 to 0: a device whitens the frame by it.
    pub fn impact(&self) -> f32 {
        if self.sim.stop > 0 && self.sim.stop_len >= 5 {
            self.sim.stop as f32 / self.sim.stop_len as f32
        } else {
            0.0
        }
    }

    /// Remote settings: `key=value` words separated by spaces.
    /// `auto hud stats world crowd mage fx govern` take 0 or 1; `lodNear lodMid lodFar repeat option pace` a number;
    /// `reset=1` restarts; `view=px,py,pz,tx,ty,tz,fov` fixes the camera and `view=off` frees it.
    pub fn control(&mut self, text: &str) {
        for word in text.split_ascii_whitespace() {
            let Some((key, value)) = word.split_once('=') else { continue };
            let on = value == "1" || value == "true";
            let num = parse_f32(value);
            match key {
                "auto" => self.set.auto = on,
                "hud" => self.set.hud = on,
                "stats" => self.set.stats = on,
                "world" => self.set.world = on,
                "crowd" => self.set.crowd = on,
                "mage" => self.set.mage = on,
                "fx" => self.set.fx = on,
                "lodNear" => self.set.lod_near = num.unwrap_or(self.set.lod_near),
                "lodMid" => self.set.lod_mid = num.unwrap_or(self.set.lod_mid),
                "lodFar" => self.set.lod_far = num.unwrap_or(self.set.lod_far),
                "repeat" => self.set.repeat = clamp(num.unwrap_or(1.0), 1.0, 8.0) as u32,
                "option" => self.set.option = num.unwrap_or(0.0) as i32,
                "pace" => self.set.pace = clamp(num.unwrap_or(PACE as f32), 1.0, 4.0) as u32,
                "govern" => self.set.govern = on,
                "reset" if on => self.sim.reset(),
                "view" => {
                    let mut f = [0.0f32; 7];
                    let mut n = 0;
                    for part in value.split(',') {
                        if let (Some(x), true) = (parse_f32(part), n < 7) {
                            f[n] = x;
                            n += 1;
                        }
                    }
                    self.set.view = if n >= 6 { Some((v3(f[0], f[1], f[2]), v3(f[3], f[4], f[5]), if n == 7 { f[6] } else { 58.0 })) } else { None };
                }
                _ => {}
            }
        }
    }

    /// The interface for this frame, laid out on the Vita's 960 × 544 and scaled to this screen.
    pub fn draw_hud(&self, h: &mut Hud) {
        let sim = &self.sim;
        let (w, ht) = (self.scene.h.screen[0], self.scene.h.screen[1]);
        let (sx, sy) = (w / 960.0, ht / 544.0);
        let sizes = &h.font.sizes;
        let (small, medium, large) = (sizes[0], sizes[sizes.len() / 2], sizes[sizes.len() - 1]);
        let r = libm::roundf;
        // A heavy strike's freeze whitens the frame for its length.
        let impact = self.impact();
        if impact > 0.0 {
            h.rect(0.0, 0.0, w, ht, rgba(214, 228, 255, (impact * 46.0) as u8));
        }
        if self.set.hud {
            let white = rgba(238, 240, 248, 255);
            let dim = rgba(238, 240, 248, 190);
            // Health and mana, bottom left.
            let hp = sim.p.hp / tune::HP_MAX;
            let (bx, by, bw, bh) = (r(30.0 * sx), r(478.0 * sy), r(264.0 * sx), max(r(14.0 * sy), 7.0));
            h.text(small, bx, by - 4.0, 0.0, dim, "MAGE");
            h.rect(bx, by, bw, bh, rgba(6, 10, 22, 150));
            h.frame(bx, by, bw, bh, 1.0, rgba(255, 255, 255, 130));
            h.rect(bx + 2.0, by + 2.0, (bw - 4.0) * hp, bh - 4.0, if hp < 0.25 { rgba(255, 110, 80, 255) } else { rgba(226, 236, 248, 255) });
            let mana = sim.p.mana / tune::MANA_MAX;
            let (my, mh) = (by + bh + 2.0, max(r(8.0 * sy), 4.0));
            h.rect(bx, my, bw, mh, rgba(6, 10, 22, 150));
            h.rect(bx + 1.0, my + 1.0, (bw - 2.0) * mana, mh - 2.0, if mana >= 1.0 { rgba(255, 226, 150, 255) } else { rgba(120, 176, 255, 255) });
            // The count, top right.
            let right = r(930.0 * sx);
            let top = r(56.0 * sy).max(large as f32 + 2.0);
            h.text(large, right, top, 1.0, white, &format!("{}", sim.p.kos));
            h.text(small, right, top + small as f32 + 3.0, 1.0, dim, &format!("of {}  -  {} stand", sim.goal, sim.crowd.standing()));
            if sim.p.chain >= 3 {
                let a = (smoothstep(tune::CHAIN_HOLD as f32, tune::CHAIN_HOLD as f32 - 40.0, sim.p.chain_t as f32) * 255.0) as u8;
                let cy = top + small as f32 + large as f32 + 12.0;
                h.text(large, right, cy, 1.0, rgba(255, 236, 190, a), &format!("{}", sim.p.chain));
                h.text(small, right, cy + small as f32 + 2.0, 1.0, rgba(255, 236, 190, a), "HITS");
            }
            if sim.p.act == act::DOWN {
                h.text(medium, w * 0.5, r(250.0 * sy), 0.5, white, "FALLEN");
            }
            if self.note.1 > 0.0 {
                let a = (min(self.note.1, 0.3) / 0.3 * 255.0) as u8;
                h.text(medium, w * 0.5, r(132.0 * sy), 0.5, rgba(238, 240, 248, a), &self.note.0);
            }
            if self.set.auto {
                if sim.tick < 420 {
                    let a = (smoothstep(420.0, 300.0, sim.tick as f32) * 255.0) as u8;
                    h.text(large, w * 0.5, r(210.0 * sy), 0.5, rgba(238, 240, 248, a), "POCKET REQUIEM");
                    h.text(small, w * 0.5, r(210.0 * sy) + small as f32 + 8.0, 0.5, rgba(238, 240, 248, a), self.help);
                }
                h.text(small, w * 0.5, ht - 8.0, 0.5, dim, "AUTOPILOT  -  press a button to take over");
            }
        }
        if self.set.stats {
            h.text(small, 6.0, small as f32 + 2.0, 0.0, rgba(255, 255, 255, 220), &self.stats_line);
        }
    }

    /// The status record as JSON. `extra` is the device's own members, without braces (may be empty).
    pub fn status(&self, out: &mut String, target: &str, perf: &Perf, extra: &str) {
        let s = &self.sim;
        let i = &self.info;
        let _ = write!(
            out,
            "{{\"target\":\"{target}\",\"stage\":\"running\",\"frame\":{},\"frameMs\":{:.3},\"worstMs\":{:.3},\"late\":{},\"frames\":{},\"cpuMs\":{{\"sim\":{:.3},\"build\":{:.3},\"draw\":{:.3}}},\"gpuMs\":{:.3},\"draws\":{},\"tris\":{},",
            self.frame, perf.frame, perf.worst, perf.late, perf.frames, perf.sim, perf.build, perf.draw, perf.gpu, perf.draws, perf.tris
        );
        let _ = write!(
            out,
            "\"settings\":{{\"auto\":{},\"hud\":{},\"stats\":{},\"world\":{},\"crowd\":{},\"mage\":{},\"fx\":{},\"lodNear\":{:.1},\"lodMid\":{:.1},\"lodFar\":{:.1},\"repeat\":{},\"option\":{},\"pace\":{},\"fixedView\":{},\"govern\":{},\"lodScale\":{:.3}}},",
            self.set.auto, self.set.hud, self.set.stats, self.set.world, self.set.crowd, self.set.mage, self.set.fx, self.set.lod_near, self.set.lod_mid, self.set.lod_far, self.set.repeat, self.set.option, self.set.pace, self.set.view.is_some(), self.set.govern, self.lod_scale
        );
        let _ = write!(
            out,
            "\"crowd\":{{\"shown\":{},\"tris\":{},\"byLod\":[{},{},{},{}],\"pulled\":{:.3},\"dropped\":{},\"far\":{}}},\"fx\":{{\"live\":{},\"verts\":{},\"tris\":{}}},\"ground\":{{\"patches\":{},\"built\":{}}},\"props\":{},",
            i.crowd.shown, i.crowd.tris, i.crowd.by_lod[0], i.crowd.by_lod[1], i.crowd.by_lod[2], i.crowd.by_lod[3], i.crowd.pulled, i.crowd.dropped, i.crowd.far, i.fx.live, i.fx.verts, (i.fx.over + i.fx.add) / 3, i.patches, i.built, i.props
        );
        let _ = write!(
            out,
            "\"player\":{{\"pos\":[{:.2},{:.2},{:.2}],\"hp\":{:.0},\"mana\":{:.1},\"kos\":{},\"chain\":{},\"tick\":{},\"standing\":{},\"goal\":{},\"won\":{},\"stop\":{}}}",
            s.p.pos.x, s.p.pos.y, s.p.pos.z, s.p.hp, s.p.mana, s.p.kos, s.p.chain, s.tick, s.crowd.standing(), s.goal, s.won, s.stop
        );
        if !extra.is_empty() {
            out.push(',');
            out.push_str(extra);
        }
        out.push('}');
    }
}

/// A number with one decimal, formatted with integer arithmetic: the float formatter in `core`
/// computes in 64 bits, which a machine without doubles does in software.
struct F1(f32);
impl core::fmt::Display for F1 {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let tenths = libm::roundf(self.0 * 10.0) as i32;
        write!(f, "{}{}.{}", if tenths < 0 { "-" } else { "" }, tenths.abs() / 10, tenths.abs() % 10)
    }
}

/// A decimal number: digits, an optional sign and one optional point.
pub fn parse_f32(s: &str) -> Option<f32> {
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    if rest.is_empty() {
        return None;
    }
    let (mut value, mut scale, mut seen_point) = (0.0f32, 1.0f32, false);
    for c in rest.bytes() {
        match c {
            b'0'..=b'9' => {
                value = value * 10.0 + (c - b'0') as f32;
                if seen_point {
                    scale *= 10.0;
                }
            }
            b'.' if !seen_point => seen_point = true,
            _ => return None,
        }
    }
    let v = value / scale;
    Some(if neg { -v } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game() -> Game {
        let (field, stage) = requiem_sim::worldfile::test_stage(4);
        let scene = Scene::new(requiem_pack::HandScene { lod_near: 40.0, lod_mid: 100.0, lod_far: 400.0, screen: [480.0, 272.0], ..Default::default() });
        Game::new(Sim::new(field, stage), scene, "")
    }

    #[test]
    fn the_governor_pulls_in_when_frames_are_late_and_lets_out_when_they_are_not() {
        let mut g = game();
        // A second in which every fifth frame is late.
        for i in 0..30 {
            g.govern(i % 5 == 0);
        }
        assert!(g.lod_scale < 1.0);
        let (near, mid, far) = g.lod();
        assert_eq!(near, 40.0);
        assert!(mid < 100.0 && mid >= near && far < 400.0);
        // Many seconds on time: back to the pack's distances.
        for _ in 0..30 * 3 * 4 {
            g.govern(false);
        }
        assert_eq!(g.lod_scale, 1.0);
        // The floor holds however late the frames are.
        for _ in 0..30 * 40 {
            g.govern(true);
        }
        assert!(g.lod_scale >= 0.5);
    }

    #[test]
    fn the_autopilot_plays_and_the_status_is_one_record() {
        let mut g = game();
        for _ in 0..600 {
            g.step(&Pad::default(), 2);
        }
        assert_eq!(g.sim.tick, 1200);
        assert!(g.sim.p.kos > 0, "the autopilot undid no binding in 20 s");
        let mut out = String::new();
        g.status(&mut out, "test", &Perf::default(), "\"x\":1");
        assert!(out.starts_with('{') && out.ends_with('}') && out.contains("\"kos\":"));
        assert_eq!(out.matches('{').count(), out.matches('}').count());
    }

    #[test]
    fn numbers_parse() {
        assert_eq!(parse_f32("12.5"), Some(12.5));
        assert_eq!(parse_f32("-3"), Some(-3.0));
        assert_eq!(parse_f32("1e3"), None);
        assert_eq!(parse_f32(""), None);
        assert_eq!(format!("{} {}", F1(16.74), F1(-0.26)), "16.7 -0.3");
    }
}
