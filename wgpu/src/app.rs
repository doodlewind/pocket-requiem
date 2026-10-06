//! The shell around the renderer: what `vita/src/main.rs` is on the console.
//!
//! A frame is the pad of the handheld the page shows, the simulation's ticks
//! (sixty a second, two a frame), the eye, the readouts, the scene. Until the
//! pack has arrived a frame says how much of it has.

use pocket_web_wgpu::gpu::{Gpu, Screen};
use pocket_web_wgpu::task;
use requiem_pack::{self as pack, Pack};
use requiem_sim::audio::Synth;
use requiem_sim::math::*;
use requiem_sim::sim::{act, btn, ev, tune, Input};
use requiem_sim::Sim;

use crate::hud::{rgba, Hud};
use crate::pack::{Coming, Ranges};
use crate::render::{Eye, Look, Parts, Renderer, Stats, Waiting};

/// PocketJS's bits of a handheld's buttons (`BTN` of `contracts/spec/spec.ts`), as the page's controls hand them in.
pub mod pad {
    pub const SELECT: u32 = 0x0001;
    pub const START: u32 = 0x0008;
    pub const UP: u32 = 0x0010;
    pub const RIGHT: u32 = 0x0020;
    pub const DOWN: u32 = 0x0040;
    pub const LEFT: u32 = 0x0080;
    pub const L: u32 = 0x0100 | 0x0400;
    pub const R: u32 = 0x0200 | 0x0800;
    /// The face button at the top, the right, the bottom and the left: △ ○ ✕ □, or X A B Y.
    pub const TOP: u32 = 0x1000;
    pub const RIGHT_FACE: u32 = 0x2000;
    pub const BOTTOM: u32 = 0x4000;
    pub const LEFT_FACE: u32 = 0x8000;
    /// The buttons that play.
    pub const PLAY: u32 = L | R | TOP | RIGHT_FACE | BOTTOM | LEFT_FACE;
}

/// A screen the page can show the game on: a handheld's size and what its own build does with a frame.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    /// Samples a pixel of the scene.
    pub samples: u32,
    /// Frames a second.
    pub hz: u32,
    /// Whether the frame goes through the PS Vita's chain: bloom, shafts, grading. A PSP's and a 3DS's do not.
    pub post: bool,
    /// The most knights out of formation at once (`crowd.free` of the handheld's profile).
    pub free: usize,
    /// The controls, as the device labels them.
    pub help: &'static str,
}

pub const SHAPES: [Shape; 3] = [
    Shape { name: "vita", width: 960, height: 544, samples: 4, hz: 30, post: true, free: requiem_sim::crowd::FREE_CAP, help: "SQUARE  strike     TRIANGLE  spell     X  evade     CIRCLE  unseal     L  guard     R  hover" },
    Shape { name: "psp", width: 480, height: 272, samples: 4, hz: 30, post: false, free: 240, help: "SQUARE strike   TRIANGLE spell   X evade   CIRCLE unseal   L guard   R hover" },
    Shape { name: "3ds", width: 400, height: 240, samples: 4, hz: 30, post: false, free: 200, help: "Y strike   X spell   B evade   A unseal   L guard   R hover" },
];

impl Shape {
    pub fn named(name: &str) -> Option<Shape> {
        SHAPES.iter().copied().find(|s| s.name == name)
    }
}

/// What is held on the page's handheld: PocketJS's button bits and two sticks in -1…1, right and up positive.
#[derive(Clone, Copy, Default)]
pub struct Held {
    pub buttons: u32,
    pub left: [f32; 2],
    pub right: [f32; 2],
}

struct Settings {
    auto: bool,
    hud: bool,
    /// Whether the autopilot's lines are on the screen while it plays: the game's name and the controls at
    /// the start, and the line that says a button takes over.
    hint: bool,
    parts: Parts,
    /// Replaces the profile's triangle budget for the army.
    crowd_budget: Option<usize>,
    look: Look,
    /// A fixed camera: eye, target, vertical field of view.
    view: Option<(V3, V3, f32)>,
    /// Presses to make, one every fourteen ticks, with the autopilot off: a chain to look at.
    cast: Vec<u32>,
    cast_wait: u32,
}

/// The game once its pack is here.
struct Game {
    renderer: Renderer,
    sim: Sim,
    synth: Synth,
    set: Settings,
    note: (String, f32),
    prev_buttons: u32,
    frame: u32,
    stats: Stats,
    /// The most knights a frame showed, and the triangles of that frame.
    most: (u32, u32),
    pack_bytes: usize,
    name: String,
    /// The bytes of the pack that have arrived.
    have: Ranges,
}

/// The field from above for a second screen: the pack's `MAPT` section of the 3DS.
struct Map {
    size: usize,
    extent: f32,
    /// The map as RGBA, row 0 at the north edge.
    ground: Vec<u8>,
}

pub struct App {
    pub gpu: Gpu,
    pub screen: Screen,
    pub shape: Shape,
    coming: Option<Coming>,
    waiting: Option<Waiting>,
    game: Option<Game>,
    map: Option<Map>,
    /// What stopped the start, said on the screen.
    pub trouble: String,
    /// Words handed in before the game was there.
    words: String,
    last: f64,
    owed: f64,
    /// Milliseconds a frame's two halves took on the processor, smoothed: the simulation, and everything handed to the GPU.
    sim_ms: f32,
    draw_ms: f32,
    load_ms: f64,
    started: f64,
}

impl App {
    /// The shell on `screen`, which has the shape's size, with no pack yet.
    pub fn open(gpu: Gpu, screen: Screen, shape: Shape) -> App {
        App { gpu, screen, shape, coming: None, waiting: None, game: None, map: None, trouble: String::new(), words: String::new(), last: -1.0, owed: 0.0, sim_ms: 0.0, draw_ms: 0.0, load_ms: 0.0, started: task::now() }
    }

    /// Starts reading the pack at `place`: its file, or the manifest of its pieces.
    pub fn read(&mut self, place: &str) {
        self.coming = Some(Coming::start(place.to_string()));
    }

    /// The field from above, for a device with a second screen: the bytes of a `MAPT` section.
    pub fn map(&mut self, section: &[u8]) -> Result<(), String> {
        let word = |at: usize| section.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let (Some(w), Some(h), Some(extent)) = (word(0), word(4), word(8)) else { return Err("the map is cut short".into()) };
        let size = w as usize;
        if w != h || section.len() < 16 + size * size * 2 {
            return Err("the map is not a square of 16-bit texels".into());
        }
        let mut ground = Vec::with_capacity(size * size * 4);
        for t in section[16..16 + size * size * 2].chunks_exact(2) {
            let c = u16::from_le_bytes([t[0], t[1]]) as u32;
            ground.extend_from_slice(&[((c >> 11) * 255 / 31) as u8, (((c >> 5) & 63) * 255 / 63) as u8, ((c & 31) * 255 / 31) as u8, 255]);
        }
        self.map = Some(Map { size, extent: f32::from_bits(extent), ground });
        Ok(())
    }

    /// Whether the game runs: its pack has arrived and is on the GPU.
    pub fn runs(&self) -> bool {
        self.game.is_some()
    }

    /// Another screen from the next frame on. A canvas has the new size already.
    pub fn reshape(&mut self, shape: Shape) {
        self.shape = shape;
        self.screen.resize(&self.gpu, shape.width, shape.height, 1);
        if let Some(game) = &mut self.game {
            game.renderer.resize(&self.gpu, shape.width, shape.height);
            game.sim.crowd.free_cap = shape.free;
            game.set.look = if shape.post { Look::DEFAULT } else { Look::PLAIN };
        }
    }

    /// What a first frame needs of the pack has arrived (`have` says which bytes): it goes to the GPU, and
    /// the fight starts.
    fn start(&mut self, bytes: Vec<u8>, have: Ranges) -> Result<(), String> {
        let p = Pack::parse(&bytes)?;
        let renderer = Renderer::new(&self.gpu, &p, &have, self.screen.format, self.shape.width, self.shape.height, self.shape.samples)?;
        let mut sim = requiem_sim::worldfile::load(p.section(pack::SIMW)?).map_err(|e| e.to_string())?;
        sim.crowd.free_cap = self.shape.free;
        let name = serde_json::from_slice::<serde_json::Value>(p.section(pack::META)?).ok().and_then(|m| m["profile"].as_str().map(String::from)).unwrap_or_default();
        let parts = Parts { world: true, crowd: true, mage: true, fx: true, lod_near: renderer.scene.lod_near, lod_mid: renderer.scene.lod_mid, crowd_scale: 1.0, crowd_budget: renderer.scene.crowd_budget, far_from: 2 };
        let set = Settings { auto: true, hud: true, hint: true, parts, crowd_budget: None, look: if self.shape.post { Look::DEFAULT } else { Look::PLAIN }, view: None, cast: Vec::new(), cast_wait: 0 };
        self.game = Some(Game { renderer, sim, synth: Synth::new(), set, note: (String::new(), 0.0), prev_buttons: u32::MAX, frame: 0, stats: Stats::default(), most: (0, 0), pack_bytes: bytes.len(), name, have });
        self.load_ms = task::now() - self.started;
        let words = std::mem::take(&mut self.words);
        self.control(&words);
        Ok(())
    }

    /// One frame at `now` (the frame loop's clock, milliseconds): the pad, the ticks, the scene.
    pub fn frame(&mut self, now: f64, held: &Held) -> Result<(), String> {
        if self.game.is_none() {
            return self.wait();
        }
        // One tick per sixtieth of a second since the last frame. A display whose refresh is no whole number
        // of sixtieths leaves a part of a tick, which is owed to the next frame.
        let due = if self.last < 0.0 { 60.0 / self.shape.hz as f64 } else { (now - self.last) * 0.06 + self.owed };
        self.last = now;
        let whole = if (due - due.round()).abs() < 0.05 { due.round() } else { due.floor() };
        self.owed = (due - whole).clamp(0.0, 1.0);
        let ticks = (whole as u32).clamp(1, 4);
        // What arrived of the pack since the last frame goes to the GPU, two reads a frame at most.
        if let (Some(coming), Some(g)) = (&self.coming, &mut self.game) {
            for _ in 0..2 {
                let Some((offset, bytes)) = coming.late() else { break };
                g.have.add(offset, offset + bytes.len() as u64);
                g.renderer.arrived(&self.gpu, &g.have, offset, &bytes);
            }
        }
        let from = task::now();
        self.step(held, ticks);
        let stepped = task::now();
        let drawn = self.draw();
        self.sim_ms = self.sim_ms * 0.9 + (stepped - from) as f32 * 0.1;
        self.draw_ms = self.draw_ms * 0.9 + (task::now() - stepped) as f32 * 0.1;
        drawn
    }

    /// A frame before the game is there: how much of the pack has arrived, or why none will.
    fn wait(&mut self) -> Result<(), String> {
        let Some(coming) = self.coming.clone() else { return Ok(()) };
        if let Some(font) = coming.font() {
            self.waiting = Some(Waiting::new(&self.gpu, &font, self.screen.format)?);
        }
        if let Some((bytes, have)) = coming.take() {
            if let Err(e) = self.start(bytes, have) {
                self.trouble = e;
            } else {
                self.waiting = None;
                return Ok(());
            }
        }
        if let (true, Some(e)) = (self.trouble.is_empty(), coming.failure()) {
            self.trouble = e;
        }
        let Some(waiting) = &mut self.waiting else { return Ok(()) };
        let (w, h) = (self.shape.width as f32, self.shape.height as f32);
        let k = h / 544.0;
        let mut hud = Hud::new(waiting.font(), k);
        let white = rgba(238, 240, 248, 255);
        let dim = rgba(238, 240, 248, 190);
        hud.text(44, w * 0.5, 230.0 * k, 0.5, white, "POCKET REQUIEM");
        if self.trouble.is_empty() {
            let (arrived, total) = coming.progress();
            let line = if total == 0 { "Reading the stage".to_string() } else if arrived >= total { "Mustering the army".to_string() } else { format!("Reading the stage: {:.1} of {:.1} MB", arrived as f32 / 1e6, total as f32 / 1e6) };
            hud.text(18, w * 0.5, 266.0 * k, 0.5, dim, &line);
            let (bx, bw) = (w * 0.5 - 132.0 * k, 264.0 * k);
            hud.rect(bx, 284.0 * k, bw, 4.0 * k, rgba(255, 255, 255, 40));
            hud.rect(bx, 284.0 * k, bw * if total > 0 { arrived as f32 / total as f32 } else { 0.0 }, 4.0 * k, rgba(226, 236, 248, 255));
        } else {
            hud.text(18, w * 0.5, 266.0 * k, 0.5, dim, "Could not start.");
            for (i, line) in self.trouble.as_bytes().chunks(90).take(3).enumerate() {
                hud.text(18, w * 0.5, (292.0 + i as f32 * 24.0) * k, 0.5, dim, &String::from_utf8_lossy(line));
            }
        }
        let frame = self.screen.frame(&self.gpu)?;
        waiting.draw(&self.gpu, &frame, self.shape.width, self.shape.height, &hud);
        frame.present();
        Ok(())
    }

    /// The pad and `ticks` ticks of the simulation.
    fn step(&mut self, held: &Held, ticks: u32) {
        let Some(g) = &mut self.game else { return };
        let buttons = held.buttons;
        let pressed = buttons & !g.prev_buttons;
        g.prev_buttons = buttons;
        let say = |g: &mut Game, text: &str, seconds: f32| g.note = (text.into(), seconds);
        if pressed & pad::SELECT != 0 && g.frame > 30 {
            g.sim.reset();
            g.set.auto = false;
            say(g, "AGAIN", 1.2);
        }
        if pressed & pad::START != 0 {
            g.set.auto = !g.set.auto;
            say(g, if g.set.auto { "AUTOPILOT" } else { "MANUAL" }, 1.5);
        }
        // Any deliberate input takes over from the autopilot.
        if g.set.auto && pressed & pad::PLAY != 0 && g.frame > 30 {
            g.set.auto = false;
            say(g, "MANUAL", 1.5);
        }
        let mut b = 0;
        for (bit, to) in [(pad::LEFT_FACE, btn::LIGHT), (pad::TOP, btn::HEAVY), (pad::BOTTOM, btn::EVADE), (pad::RIGHT_FACE, btn::UNSEAL), (pad::L, btn::GUARD), (pad::R, btn::HOVER)] {
            if buttons & bit != 0 {
                b |= to;
            }
        }
        // The camera: a second stick, and the direction pad as on a machine with one stick.
        let dir = |less: u32, more: u32| (buttons & more != 0) as i32 as f32 - (buttons & less != 0) as i32 as f32;
        let own = Input { buttons: b, lx: held.left[0], ly: held.left[1], rx: clamp(held.right[0] + dir(pad::LEFT, pad::RIGHT), -1.0, 1.0), ry: clamp(held.right[1] + dir(pad::DOWN, pad::UP), -1.0, 1.0) };
        let mut events = 0u32;
        for _ in 0..ticks {
            let mut inp = if g.set.auto { g.sim.auto_input() } else { own };
            if !g.set.cast.is_empty() {
                if g.set.cast_wait == 0 {
                    inp.buttons |= g.set.cast.pop().unwrap_or(0);
                    g.set.cast_wait = 14;
                } else {
                    g.set.cast_wait -= 1;
                }
            }
            g.sim.tick(inp);
            events |= g.sim.events;
            g.synth.control(&g.sim, g.sim.events);
        }
        for (event, text, seconds) in [(ev::READY, "MANA FULL", 1.4), (ev::UNSEAL, "THE BINDING COMES UNDONE", 2.2), (ev::FALLEN, "FALLEN", 2.5), (ev::WON, "EVERY BINDING IS UNDONE.", 6.0)] {
            if events & event != 0 {
                say(g, text, seconds);
            }
        }
        g.note.1 -= ticks as f32 / 60.0;
        g.frame = g.frame.wrapping_add(1);
    }

    /// The scene and the readouts onto the screen.
    fn draw(&mut self) -> Result<(), String> {
        let shape = self.shape;
        let Some(g) = &mut self.game else { return Ok(()) };
        let sim = &g.sim;
        let eye = match g.set.view {
            Some((pos, target, fov)) => Eye { pos, look: (target - pos).norm_or(v3(0.0, 0.0, -1.0)), fov },
            None => {
                let k = sim.cam.shake * 0.22;
                let t = sim.tick as f32;
                Eye { pos: sim.cam.pos + v3(sin(t * 1.7) * k, sin(t * 2.3) * k, cos(t * 1.9) * k), look: sim.cam.look, fov: sim.cam.fov }
            }
        };
        // A heavy strike holds the frame: while it does, the picture hardens, drains and pulls toward its centre.
        let impact = if sim.stop > 0 && sim.stop_len >= 5 { sim.stop as f32 / sim.stop_len as f32 } else { 0.0 };
        let mut look = g.set.look;
        if shape.post {
            look.contrast += 0.55 * impact;
            look.saturation -= 0.6 * impact;
            look.bloom_gain += 0.6 * impact;
            look.vignette += 0.25 * impact;
        }
        let fast = max(sim.p.hover * 0.6, impact * 0.9) * if g.set.view.is_some() { 0.0 } else { 1.0 };
        let (w, h) = (shape.width as f32, shape.height as f32);
        let mut hud = Hud::new(g.renderer.font(), h / 544.0);
        // A handheld without the chain whitens the frame for the length of the freeze.
        if !shape.post && impact > 0.0 {
            hud.rect(0.0, 0.0, w, h, rgba(214, 228, 255, (impact * 46.0) as u8));
        }
        if g.set.hud {
            readouts(&mut hud, sim, &g.note, g.set.auto && g.set.hint, w, h, shape.help);
        }
        let mut parts = g.set.parts;
        parts.crowd_budget = g.set.crowd_budget.unwrap_or(parts.crowd_budget);
        let frame = self.screen.frame(&self.gpu)?;
        g.stats = g.renderer.draw(&self.gpu, &frame, &g.sim, &eye, &parts, &look, fast, &hud);
        frame.present();
        if g.stats.crowd.shown > g.most.0 {
            g.most = (g.stats.crowd.shown, g.stats.crowd.tris);
        }
        Ok(())
    }

    /// `frames` frames of sound at `rate` a second, as pairs of left and right: what the ticks since the last
    /// call sounded like, and the wind of the field under them.
    pub fn sound(&mut self, frames: usize, rate: f32) -> Vec<i16> {
        let mut pcm = vec![0i16; frames * 2];
        if let Some(g) = &mut self.game {
            g.synth.render(&mut pcm, rate);
        }
        pcm
    }

    /// The second screen's map as RGBA, `size` rows of `size`: the field from above, a mark for every cohort
    /// still in formation, the demon, and the mage with her heading. Empty without a map.
    pub fn lower(&self) -> (Vec<u8>, usize) {
        let (Some(map), Some(g)) = (&self.map, &self.game) else { return (Vec::new(), 0) };
        let n = map.size as i32;
        let mut out = map.ground.clone();
        let mut plot = |x: i32, y: i32, c: [u8; 3]| {
            if x >= 0 && y >= 0 && x < n && y < n {
                out[(y * n + x) as usize * 4..(y * n + x) as usize * 4 + 3].copy_from_slice(&c);
            }
        };
        let k = map.size as f32 / (2.0 * map.extent);
        let at = |v: f32| ((v + map.extent) * k) as i32;
        let mut dot = |x: i32, y: i32, r: i32, c: [u8; 3]| {
            for j in -r - 1..=r + 1 {
                for i in -r - 1..=r + 1 {
                    plot(x + i, y + j, if i.abs() > r || j.abs() > r { [0, 0, 0] } else { c });
                }
            }
        };
        let s = &g.sim;
        for c in s.crowd.cohorts.iter().filter(|c| c.formed > 0) {
            dot(at(c.x), at(c.z), 1, [156, 174, 255]);
        }
        dot(at(s.stage.demon.0), at(s.stage.demon.1), 2, [255, 0, 255]);
        let (px, py) = (at(s.p.pos.x), at(s.p.pos.z));
        // The heading: a short line from the mark.
        let (dx, dy) = (-sin(s.cam.yaw), -cos(s.cam.yaw));
        for t in 3..=8 {
            dot(px + (dx * t as f32) as i32, py + (dy * t as f32) as i32, 0, [255, 255, 255]);
        }
        dot(px, py, 2, [255, 255, 0]);
        (out, map.size)
    }

    /// Words for the run, as a development host sends them: `auto hud hint world crowd mage fx bloom rays speed`
    /// take 0 or 1; `lodNear lodMid crowdBudget crowdScale farFrom free` a number; `reset=1` starts again;
    /// `view=px,py,pz,tx,ty,tz,fov` fixes the eye and `view=off` frees it; `cast=1,1,2` presses the
    /// simulation's buttons one after another with the autopilot off; `skip=600` runs that many ticks unseen.
    pub fn control(&mut self, words: &str) {
        let Some(g) = &mut self.game else {
            self.words = format!("{} {words}", self.words);
            return;
        };
        for word in words.split_ascii_whitespace() {
            let Some((key, value)) = word.split_once('=') else { continue };
            let on = value == "1" || value == "true";
            let num = value.parse::<f32>().ok();
            let s = &mut g.set;
            match key {
                "auto" => s.auto = on,
                "hud" => s.hud = on,
                "hint" => s.hint = on,
                "world" => s.parts.world = on,
                "crowd" => s.parts.crowd = on,
                "mage" => s.parts.mage = on,
                "fx" => s.parts.fx = on,
                "bloom" => s.look.bloom = on,
                "rays" => s.look.rays = on,
                "speed" => s.look.speed = on,
                "lodNear" => s.parts.lod_near = num.unwrap_or(s.parts.lod_near),
                "lodMid" => s.parts.lod_mid = num.unwrap_or(s.parts.lod_mid),
                "crowdBudget" => s.crowd_budget = num.map(|x| x as usize),
                "crowdScale" => s.parts.crowd_scale = clamp(num.unwrap_or(1.0), 0.1, 4.0),
                "farFrom" => s.parts.far_from = num.unwrap_or(2.0) as usize,
                "free" => g.sim.crowd.free_cap = num.unwrap_or(self.shape.free as f32) as usize,
                "reset" if on => g.sim.reset(),
                "skip" => {
                    for _ in 0..num.unwrap_or(0.0) as u32 {
                        let inp = if s.auto { g.sim.auto_input() } else { Input::default() };
                        g.sim.tick(inp);
                        g.synth.control(&g.sim, g.sim.events);
                    }
                }
                "cast" => {
                    s.cast = value.split(',').rev().filter_map(|b| b.parse().ok()).collect();
                    s.cast_wait = 0;
                    s.auto = false;
                }
                "view" => {
                    let f: Vec<f32> = value.split(',').filter_map(|x| x.parse().ok()).collect();
                    s.view = (f.len() >= 6).then(|| (v3(f[0], f[1], f[2]), v3(f[3], f[4], f[5]), f.get(6).copied().unwrap_or(58.0)));
                }
                _ => {}
            }
        }
    }

    /// The run as a JSON object.
    pub fn status(&self) -> String {
        let (arrived, total) = self.coming.as_ref().map(|c| c.progress()).unwrap_or_default();
        let (all, whole) = self.coming.as_ref().map(|c| c.streamed()).unwrap_or_default();
        let waiting = self.game.as_ref().map(|g| g.renderer.waiting()).unwrap_or_default();
        let head = format!(
            "\"shape\":\"{}\",\"size\":[{},{}],\"hz\":{},\"adapter\":{:?},\"read\":{{\"first\":[{arrived},{total}],\"all\":[{all},{whole}],\"meshesWaiting\":{},\"levelsWaiting\":{}}},\"trouble\":{:?}",
            self.shape.name, self.shape.width, self.shape.height, self.shape.hz, self.gpu.adapter, waiting.0, waiting.1, self.trouble
        );
        let Some(g) = &self.game else { return format!("{{\"stage\":\"reading\",{head}}}") };
        let (s, st) = (&g.sim, &g.stats);
        format!(
            "{{\"stage\":\"running\",{head},\"pack\":{{\"bytes\":{},\"profile\":{:?},\"onGpu\":{}}},\"loadMs\":{:.0},\"frame\":{},\"cpuMs\":{{\"sim\":{:.3},\"draw\":{:.3}}},\"tris\":{},\"draws\":{},\
             \"world\":{{\"draws\":{},\"tris\":{},\"near\":{},\"mid\":{},\"far\":{},\"lit\":{}}},\
             \"crowd\":{{\"shown\":{},\"draws\":{},\"tris\":{},\"byLod\":{:?},\"pulled\":{:.3},\"free\":{},\"freeCap\":{},\"standing\":{},\"mostShown\":{},\"mostTris\":{}}},\
             \"figures\":{{\"tris\":{}}},\"fx\":{{\"live\":{},\"draws\":{},\"tris\":{}}},\
             \"settings\":{{\"auto\":{},\"hud\":{},\"post\":{},\"fixedView\":{}}},\
             \"player\":{{\"pos\":[{:.2},{:.2},{:.2}],\"act\":{},\"move\":{},\"hp\":{:.0},\"mana\":{:.1},\"kos\":{},\"chain\":{},\"tick\":{},\"stop\":{},\"goal\":{},\"won\":{}}}}}",
            g.pack_bytes, g.name, g.renderer.bytes, self.load_ms, g.frame, self.sim_ms, self.draw_ms, st.tris(), st.draws(),
            st.world.draws, st.world.tris, st.world.near, st.world.mid, st.world.far, st.world.lit,
            st.crowd.shown, st.crowd.draws, st.crowd.tris, st.crowd.by_lod, st.crowd.pulled, s.crowd.free.len(), s.crowd.free_cap, s.crowd.standing(), g.most.0, g.most.1,
            st.figures, st.fx.live, st.fx.draws, st.fx.tris,
            g.set.auto, g.set.hud, self.shape.post, g.set.view.is_some(),
            s.p.pos.x, s.p.pos.y, s.p.pos.z, s.p.act, s.p.mv, s.p.hp, s.p.mana, s.p.kos, s.p.chain, s.tick, s.stop, s.goal, s.won
        )
    }

    /// The fight in numbers, for a second screen's text: health and mana as shares of the whole, the count,
    /// its goal, the knights still standing, and whether the autopilot plays and the fight is won.
    pub fn numbers(&self) -> Option<(f32, f32, u32, u32, u32, bool, bool)> {
        let g = self.game.as_ref()?;
        Some((g.sim.p.hp / tune::HP_MAX, g.sim.p.mana / tune::MANA_MAX, g.sim.p.kos, g.sim.goal, g.sim.crowd.standing(), g.set.auto, g.sim.won))
    }
}

/// The readouts of a frame, laid out on the PS Vita's 960 × 544 (`draw_hud` of `vita/src/main.rs`) and
/// placed on a screen of `w × h`.
fn readouts(h: &mut Hud, sim: &Sim, note: &(String, f32), auto: bool, w: f32, ht: f32, help: &str) {
    let (sx, sy) = (w / 960.0, ht / 544.0);
    let k = h.scale;
    let white = rgba(238, 240, 248, 255);
    let dim = rgba(238, 240, 248, 190);
    // Health and mana, bottom left.
    let hp = sim.p.hp / tune::HP_MAX;
    h.text(18, 30.0 * sx, 470.0 * sy, 0.0, dim, "MAGE");
    h.rect(30.0 * sx, 478.0 * sy, 264.0 * sx, 14.0 * sy, rgba(6, 10, 22, 150));
    h.frame(30.0 * sx, 478.0 * sy, 264.0 * sx, 14.0 * sy, max(1.5 * k, 1.0), rgba(255, 255, 255, 130));
    h.rect(33.0 * sx, 481.0 * sy, 258.0 * sx * hp, 8.0 * sy, if hp < 0.25 { rgba(255, 110, 80, 255) } else { rgba(226, 236, 248, 255) });
    let mana = sim.p.mana / tune::MANA_MAX;
    h.rect(30.0 * sx, 496.0 * sy, 264.0 * sx, 8.0 * sy, rgba(6, 10, 22, 150));
    h.rect(32.0 * sx, 498.0 * sy, 260.0 * sx * mana, 4.0 * sy, if mana >= 1.0 { rgba(255, 226, 150, 255) } else { rgba(120, 176, 255, 255) });
    // The count, top right.
    h.text(44, 930.0 * sx, 56.0 * sy, 1.0, white, &format!("{}", sim.p.kos));
    h.text(18, 930.0 * sx, 80.0 * sy, 1.0, dim, &format!("of {}  -  {} stand", sim.goal, sim.crowd.standing()));
    if sim.p.chain >= 3 {
        let a = (smoothstep(tune::CHAIN_HOLD as f32, tune::CHAIN_HOLD as f32 - 40.0, sim.p.chain_t as f32) * 255.0) as u8;
        h.text(44, 930.0 * sx, 150.0 * sy, 1.0, rgba(255, 236, 190, a), &format!("{}", sim.p.chain));
        h.text(18, 930.0 * sx, 172.0 * sy, 1.0, rgba(255, 236, 190, a), "HITS");
    }
    if sim.p.act == act::DOWN {
        h.text(26, w * 0.5, 250.0 * sy, 0.5, white, "FALLEN");
    }
    if note.1 > 0.0 {
        let a = (note.1.min(0.3) / 0.3 * 255.0) as u8;
        h.text(26, w * 0.5, 132.0 * sy, 0.5, rgba(238, 240, 248, a), &note.0);
    }
    if auto {
        if sim.tick < 420 {
            let a = (smoothstep(420.0, 300.0, sim.tick as f32) * 255.0) as u8;
            h.text(44, w * 0.5, 210.0 * sy, 0.5, rgba(238, 240, 248, a), "POCKET REQUIEM");
            h.text(18, w * 0.5, 240.0 * sy, 0.5, rgba(238, 240, 248, a), help);
        }
        h.text(18, w * 0.5, 528.0 * sy, 0.5, dim, "AUTOPILOT  -  press a button to take over");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_is_a_handheld_of_thirty_frames() {
        for shape in SHAPES {
            assert_eq!(Shape::named(shape.name).map(|s| (s.width, s.height)), Some((shape.width, shape.height)));
            assert_eq!(shape.hz, 30);
            assert!(shape.free <= requiem_sim::crowd::FREE_CAP);
        }
        // The PS Vita's frame has the chain; a PSP's and a 3DS's do not.
        assert_eq!(SHAPES.map(|s| s.post), [true, false, false]);
        assert!(Shape::named("ipod").is_none());
    }

    #[test]
    fn the_buttons_that_play_are_the_diamond_and_the_shoulders() {
        assert_eq!(pad::PLAY & (pad::START | pad::SELECT | pad::UP | pad::DOWN | pad::LEFT | pad::RIGHT), 0);
        assert_eq!(pad::PLAY.count_ones(), 8);
    }
}
