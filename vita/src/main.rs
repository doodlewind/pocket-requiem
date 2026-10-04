//! Pocket Requiem on PS Vita.
//!
//! A frame at 960 × 544, thirty times a second: the night sky, the baked
//! field, the army as instanced blends of stored frames, the mage skinned on
//! the GPU, then the post-processing chain and the interface. The simulation
//! is `requiem-sim`, the same crate the reference runs as wasm; it ticks
//! sixty times a second, twice per frame.
//!
//! Development loop over PocketJS's wired debug transport: the pack is read
//! from the USB share (`host0:requiem/stage.pack`), `host0:requiem/control.json`
//! steers the run, and status receipts carry frame timings under `engine`.

mod crowd;
mod figures;
mod fx;
mod gpu;
mod hostfs;
mod hud;
mod mat;
mod paths;
mod post;
mod world;

use std::io::Read;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crowd::Crowd;
use figures::{Figures, Lit, Scene};
use gpu::{Gpu, Layout, Stream};
use hud::{rgba, Hud};
use pocket_vita_gxm::mem::{Arena, Kind, Ring};
use pocket_vita_gxm::program::{F32, S16N, S8N, U16N, U8, U8N};
use pocket_vita_gxm::target::{Fence, Msaa};
use pocketjs_vita::{dev, dev_protocol::Op, devmenu::Action, graphics, input};
use post::{Look, Post};
use requiem_pack::{self as pack, Pack};
use requiem_sim::math::*;
use requiem_sim::sim::{act, btn, ev, tune, Input};
use requiem_sim::Sim;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use vita2d_sys as g;

#[no_mangle]
#[used]
pub static sceUserMainThreadStackSize: u32 = 1024 * 1024;

#[no_mangle]
#[used]
pub static _newlib_heap_size_user: u32 = 100 * 1024 * 1024;

extern "C" {
    fn scePowerSetArmClockFrequency(freq: i32) -> i32;
    fn scePowerSetBusClockFrequency(freq: i32) -> i32;
    fn scePowerSetGpuClockFrequency(freq: i32) -> i32;
    fn scePowerSetGpuXbarClockFrequency(freq: i32) -> i32;
    fn scePowerGetArmClockFrequency() -> i32;
    fn scePowerGetGpuClockFrequency() -> i32;
    fn sceDisplayGetVcount() -> i32;
    fn sceDisplayWaitVblankStart() -> i32;
}

/// Samples per pixel of the scene target.
const DEFAULT_MSAA: u64 = 4;

/// ARM, bus, GPU and GPU crossbar clocks (MHz) the frame budget assumes.
const CLOCKS: [i32; 4] = [444, 222, 222, 166];

unsafe fn set_clocks() {
    scePowerSetArmClockFrequency(CLOCKS[0]);
    scePowerSetBusClockFrequency(CLOCKS[1]);
    scePowerSetGpuClockFrequency(CLOCKS[2]);
    scePowerSetGpuXbarClockFrequency(CLOCKS[3]);
}

const WORLD_V: &str = include_str!("../shaders/world_v.cg");
const WORLD_F: &str = include_str!("../shaders/world_f.cg");
const WORLD_LIT_V: &str = include_str!("../shaders/world_lit_v.cg");
const COLOR_V: &str = include_str!("../shaders/color_v.cg");
const COLOR_F: &str = include_str!("../shaders/color_f.cg");
const SKIN_V: &str = include_str!("../shaders/skin_v.cg");
const CROWD_V: &str = include_str!("../shaders/crowd_v.cg");
const FX_HEAD: &str = include_str!("../shaders/fx_head.cg");
const FX_F: &str = include_str!("../shaders/fx_f.cg");
const FX_V: [(&str, &str); 4] = [
    ("fx_particles", include_str!("../shaders/fx_particles_v.cg")),
    ("fx_ring", include_str!("../shaders/fx_ring_v.cg")),
    ("fx_ribbon", include_str!("../shaders/fx_ribbon_v.cg")),
    ("fx_shell", include_str!("../shaders/fx_shell_v.cg")),
];
const HUD_V: &str = include_str!("../shaders/hud_v.cg");
const HUD_F: &str = include_str!("../shaders/hud_f.cg");

// Vita controller bits.
const P_SELECT: u32 = 0x1;
const P_START: u32 = 0x8;
const P_L: u32 = 0x100 | 0x400;
const P_R: u32 = 0x200 | 0x800;
const P_TRIANGLE: u32 = 0x1000;
const P_CIRCLE: u32 = 0x2000;
const P_CROSS: u32 = 0x4000;
const P_SQUARE: u32 = 0x8000;

unsafe fn text(font: *mut g::vita2d_pgf, x: i32, y: i32, color: u32, scale: f32, s: &str) {
    let c = std::ffi::CString::new(s.replace('\0', " ")).unwrap();
    g::vita2d_pgf_draw_text(font, x, y, color, scale, c.as_ptr());
}

/// A frame of the loading screen; it also publishes status, so the computer sees the new process come up.
unsafe fn loading(font: *mut g::vita2d_pgf, dev: &mut dev::Host, frame: &mut u32, lines: &[String]) {
    graphics::begin_frame(0xff14_100c);
    text(font, 48, 80, 0xffff_ffff, 1.4, "Pocket Requiem");
    for (i, l) in lines.iter().enumerate() {
        text(font, 48, 130 + i as i32 * 28, 0xffd0_d0d0, 1.0, l);
    }
    dev.overlay();
    graphics::present();
    dev.engine = json!({"stage": "loading", "lines": lines});
    dev.publish(*frame, "requiem");
    serve(dev, *frame, Action::None);
    *frame += 1;
}

/// Reads the pack in pieces, drawing the loading screen between them.
unsafe fn read_pack(font: *mut g::vita2d_pgf, dev: &mut dev::Host, frame: &mut u32) -> Result<(Vec<u8>, &'static str, String), String> {
    let mut last = String::new();
    for path in paths::PACKS {
        let mut file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) => {
                last = format!("{path}: {e}");
                continue;
            }
        };
        // The section table says how long the pack is: reserve it once, so the buffer never doubles.
        let mut head = [0u8; 16];
        file.read_exact(&mut head).map_err(|e| format!("{path}: {e}"))?;
        let count = u32::from_le_bytes([head[8], head[9], head[10], head[11]]) as usize;
        let mut table = vec![0u8; count.min(64) * 16];
        file.read_exact(&mut table).map_err(|e| format!("{path}: {e}"))?;
        let total = table.chunks_exact(16).map(|e| u32::from_le_bytes([e[4], e[5], e[6], e[7]]) as usize + u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize).max().unwrap_or(0);
        let mut bytes = Vec::with_capacity(total + 16);
        let mut hash = Sha256::new();
        bytes.extend_from_slice(&head);
        bytes.extend_from_slice(&table);
        hash.update(&head);
        hash.update(&table);
        let mut chunk = vec![0u8; 512 * 1024];
        let t = Instant::now();
        loop {
            let n = file.read(&mut chunk).map_err(|e| format!("{path}: {e}"))?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
            hash.update(&chunk[..n]);
            let secs = t.elapsed().as_secs_f32().max(0.001);
            loading(font, dev, frame, &[format!("Reading the stage: {:.1} MB", bytes.len() as f32 / 1e6), format!("{:.2} MB/s from {path}", bytes.len() as f32 / 1e6 / secs)]);
        }
        let sha: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        return Ok((bytes, path, sha));
    }
    Err(format!("no world pack ({last})"))
}

/// Copies `host0:requiem/outbox/<name>` to `ux0:data/pocket-requiem/` (a packaged build to
/// install from VitaShell) and records the result next to the source as `<name>.done`.
fn fetch(name: &str) {
    let result = (|| -> Result<u64, String> {
        if name.is_empty() || name.contains(['/', '\\', ':']) || name.contains("..") {
            return Err(format!("refusing file name {name:?}"));
        }
        let _ = std::fs::create_dir_all(paths::DATA);
        let mut src = std::fs::File::open(format!("{}/outbox/{name}", paths::HOST)).map_err(|e| e.to_string())?;
        let to = format!("{}/{name}", paths::DATA);
        let mut dst = std::fs::File::create(&to).map_err(|e| format!("{to}: {e}"))?;
        std::io::copy(&mut src, &mut dst).map_err(|e| e.to_string())
    })();
    let text = match result {
        Ok(n) => format!("ok {n} {}/{name}", paths::DATA),
        Err(e) => format!("error {e}"),
    };
    let _ = hostfs::write(&format!("{}/outbox/{name}.done", paths::HOST), text.as_bytes());
}

/// Remote control: `host0:requiem/control.json`, polled off the render thread.
fn control_watcher() -> mpsc::Receiver<Value> {
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new().name("requiem-control".into()).stack_size(256 * 1024).spawn(move || {
        // What is there at launch is left over from an earlier run: only changes after it count.
        let path = format!("{}/control.json", paths::HOST);
        let mut last = hostfs::read(&path, 64 * 1024).unwrap_or_default();
        loop {
            std::thread::sleep(Duration::from_millis(300));
            if let Some(bytes) = hostfs::read(&path, 64 * 1024) {
                if bytes != last {
                    last = bytes.clone();
                    if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                        if let Some(name) = v["fetch"].as_str() {
                            fetch(name);
                        }
                        if tx.send(v).is_err() {
                            return;
                        }
                    }
                }
            }
        }
    });
    rx
}

struct Settings {
    auto: bool,
    hud: bool,
    stats: bool,
    cull_cw: bool,
    /// Wait for the GPU after each scene and time it.
    profile: bool,
    lod_near: f32,
    lod_mid: f32,
    world: bool,
    crowd: bool,
    mage: bool,
    fx: bool,
    /// Replaces the profile's triangle budget for the army.
    crowd_budget: Option<usize>,
    /// Scales the distances at which the army's levels of detail hand over.
    crowd_scale: f32,
    /// The first level of detail drawn with the far program.
    far_from: usize,
    /// Display refreshes per frame: 2 is thirty frames a second.
    pace: i32,
    look: Look,
    /// A fixed camera: eye, target, vertical field of view.
    view: Option<(V3, V3, f32)>,
    /// Presses to make, one every fourteen ticks, with the autopilot off: a chain to look at.
    cast: Vec<u32>,
    cast_wait: u32,
}

fn apply_control(v: &Value, s: &mut Settings, sim: &mut Sim) {
    let flag = |k: &str, cur: bool| v[k].as_bool().unwrap_or(cur);
    s.auto = flag("auto", s.auto);
    s.hud = flag("hud", s.hud);
    s.stats = flag("stats", s.stats);
    s.cull_cw = flag("cullCw", s.cull_cw);
    s.profile = flag("profile", s.profile);
    s.world = flag("world", s.world);
    s.crowd = flag("crowd", s.crowd);
    s.mage = flag("mage", s.mage);
    s.fx = flag("fx", s.fx);
    if let Some(x) = v["lodNear"].as_f64() {
        s.lod_near = x as f32;
    }
    if let Some(x) = v["lodMid"].as_f64() {
        s.lod_mid = x as f32;
    }
    if let Some(x) = v["crowdBudget"].as_u64() {
        s.crowd_budget = Some(x as usize);
    }
    if let Some(x) = v["crowdScale"].as_f64() {
        s.crowd_scale = (x as f32).clamp(0.1, 4.0);
    }
    if let Some(x) = v["farFrom"].as_u64() {
        s.far_from = x as usize;
    }
    if let Some(x) = v["pace"].as_i64() {
        s.pace = (x as i32).clamp(1, 4);
    }
    // `post`: {"bloom", "rays", "speed"} switch the passes; the rest are the look's numbers.
    let post = &v["post"];
    if post.is_object() {
        let l = &mut s.look;
        l.bloom = post["bloom"].as_bool().unwrap_or(l.bloom);
        l.rays = post["rays"].as_bool().unwrap_or(l.rays);
        l.speed = post["speed"].as_bool().unwrap_or(l.speed);
        for (key, slot) in [("threshold", &mut l.threshold), ("bloomGain", &mut l.bloom_gain), ("raysGain", &mut l.rays_gain), ("vignette", &mut l.vignette), ("contrast", &mut l.contrast), ("saturation", &mut l.saturation), ("warm", &mut l.warm), ("cool", &mut l.cool)] {
            if let Some(x) = post[key].as_f64() {
                *slot = x as f32;
            }
        }
    }
    if v["reset"].as_bool() == Some(true) {
        sim.reset();
    }
    if let Some(list) = v["cast"].as_array() {
        s.cast = list.iter().rev().filter_map(|b| b.as_u64().map(|b| b as u32)).collect();
        s.cast_wait = 0;
        s.auto = false;
    }
    let f = |a: &Value, i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    s.view = match (&v["view"]["pos"], &v["view"]["target"]) {
        (p, t) if p.is_array() && t.is_array() => Some((v3(f(p, 0), f(p, 1), f(p, 2)), v3(f(t, 0), f(t, 1), f(t, 2)), v["view"]["fov"].as_f64().unwrap_or(58.0) as f32)),
        _ => None,
    };
}

fn pad_input(pad: &input::Pad) -> Input {
    let mut b = 0;
    for (bit, to) in [(P_SQUARE, btn::LIGHT), (P_TRIANGLE, btn::HEAVY), (P_CROSS, btn::EVADE), (P_CIRCLE, btn::UNSEAL), (P_L, btn::GUARD), (P_R, btn::HOVER)] {
        if pad.buttons & bit != 0 {
            b |= to;
        }
    }
    let axis = |v: u8| {
        let x = (v as f32 - 127.5) / 127.5;
        // The sticks rest off centre by up to a fifth of their travel: nothing inside that counts, and the rest is rescaled.
        const DEAD: f32 = 0.24;
        if abs(x) < DEAD {
            0.0
        } else {
            (x - DEAD * if x < 0.0 { -1.0 } else { 1.0 }) / (1.0 - DEAD)
        }
    };
    Input { buttons: b, lx: axis(pad.lx), ly: -axis(pad.ly), rx: axis(pad.rx), ry: -axis(pad.ry) }
}

/// Rolling frame statistics over the last `N` frames.
struct Timing {
    ms: [f32; Timing::N],
    at: usize,
    late: u32,
    frames: u32,
}

impl Timing {
    const N: usize = 240;
    fn push(&mut self, ms: f32, vblanks: i32) {
        self.ms[self.at] = ms;
        self.at = (self.at + 1) % Self::N;
        self.frames += 1;
        if vblanks > 1 {
            self.late += 1;
        }
    }
    fn avg(&self) -> f32 {
        self.ms.iter().sum::<f32>() / Self::N as f32
    }
    fn worst(&self) -> f32 {
        self.ms.iter().fold(0.0, |a, &b| a.max(b))
    }
}

fn main() {
    unsafe {
        // Development builds take boot switches from the USB share: {"msaa": 0 | 2 | 4, "title": false}.
        let live = cfg!(feature = "usb-debug");
        let boot: Value = if live { hostfs::read(&format!("{}/boot.json", paths::HOST), 4096).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null) } else { Value::Null };
        let samples = boot["msaa"].as_u64().unwrap_or(DEFAULT_MSAA);
        // The scene target is multisampled; the display surface it is composed onto is not.
        let msaa = match samples {
            4 => Msaa::X4,
            2 => Msaa::X2,
            _ => Msaa::None,
        };
        // The Pocket3D title card plays before the renderer starts. A development build skips it with {"title": false}.
        if !(live && boot["title"] == Value::Bool(false)) {
            pocket3d_title::vita::play();
        }
        if let Err(error) = graphics::init_with_pool(1024 * 1024) {
            pocketjs_vita::vita_log(format_args!("requiem: graphics {error}"));
            return;
        }
        set_clocks();
        input::init();
        let mut dev = dev::Host::new();
        let font = g::vita2d_load_default_pgf();
        let mut frame_no = 0u32;
        let fail = |font, dev: &mut dev::Host, frame_no: &mut u32, e: String| -> ! {
            pocketjs_vita::vita_log(format_args!("requiem: {e}"));
            loop {
                loading(font, dev, frame_no, &["Could not start.".into(), e.chars().take(90).collect(), e.chars().skip(90).take(90).collect()]);
                std::thread::sleep(Duration::from_millis(100));
            }
        };

        // ------------------------------------------------------------------ load
        let t_load = Instant::now();
        loading(font, &mut dev, &mut frame_no, &["Reading the stage".into()]);
        let (bytes, pack_path, pack_sha) = match read_pack(font, &mut dev, &mut frame_no) {
            Ok(b) => b,
            Err(e) => fail(font, &mut dev, &mut frame_no, e),
        };
        let read_ms = t_load.elapsed().as_millis() as u64;
        let loaded = (|| -> Result<_, String> {
            let p = Pack::parse(&bytes)?;
            let meta: Value = serde_json::from_slice(p.section(pack::META)?).map_err(|e| e.to_string())?;
            let scene = Scene::from_meta(&meta);

            loading(font, &mut dev, &mut frame_no, &["Preparing programs".into()]);
            let mut gpu = Gpu::new(live)?;
            let fog = scene.fog_srgb();
            let crowd_head: pack::CrowdHeader = pack::read(p.section(pack::CRWD)?, 0).ok_or("crowd header")?;
            let defines = format!(
                "#define FOG_COLOR half3({:.5}, {:.5}, {:.5})\n#define FOG_DENSITY {:.7}\n#define UV_SCALE {:.1}\n#define COLOR_SCALE {:.1}\n#define BONES {}\n#define POS_SCALE {:.4}\n",
                fog[0],
                fog[1],
                fog[2],
                scene.fog_density,
                pack::UV_SCALE,
                pack::COLOR_SCALE,
                requiem_sim::skel::BONES,
                crowd_head.scale
            );
            let world_prog = gpu.program("world", &defines, WORLD_V, WORLD_F, &Layout { attrs: &[("aPosition", 0, U16N, 3), ("aUv", 8, S16N, 2), ("aColor", 12, U8N, 4)], stride: 16 }, msaa.gxm())?;
            let world_lit_prog = gpu.program("world_lit", &format!("{defines}#define LIGHT_GAIN float3(4.0, 2.8, 1.43)\n"), WORLD_LIT_V, WORLD_F, &Layout { attrs: &[("aPosition", 0, U16N, 3), ("aUv", 8, S16N, 2), ("aColor", 12, U8N, 4)], stride: 16 }, msaa.gxm())?;
            let color_prog = gpu.program("color", &defines, COLOR_V, COLOR_F, &Layout { attrs: &[("aPosition", 0, F32, 3), ("aColor", 12, U8N, 4)], stride: 16 }, msaa.gxm())?;
            let skin_prog = gpu.program("skin", &defines, SKIN_V, COLOR_F, &Layout { attrs: &[("aPosition", 0, F32, 3), ("aNormal", 12, S8N, 4), ("aColor", 16, U8N, 4), ("aBones", 20, U8, 2), ("aWeights", 22, U8N, 2)], stride: 24 }, msaa.gxm())?;
            // The army: two streams of stored frames, one of colours, one record per knight.
            let frame_attrs = |pos: &'static str, nrm: &'static str| [(pos, 0u16, S16N, 3u8), (nrm, 8, S8N, 4)];
            let (fa, fb) = (frame_attrs("aPosA", "aNrmA"), frame_attrs("aPosB", "aNrmB"));
            let crowd_prog = gpu.program_streams(
                "crowd",
                &defines,
                CROWD_V,
                COLOR_F,
                &[
                    Stream { stride: 12, instanced: false, attrs: &fa },
                    Stream { stride: 12, instanced: false, attrs: &fb },
                    Stream { stride: 4, instanced: false, attrs: &[("aColor", 0, U8N, 4)] },
                    Stream { stride: 20, instanced: true, attrs: &[("iPos", 0, F32, 3), ("iTurn", 12, S16N, 2), ("iMisc", 16, U8N, 4)] },
                ],
                msaa.gxm(),
            )?;
            let crowd_far_prog = gpu.program_streams(
                "crowd_far",
                &format!("{defines}#define FAR\n"),
                CROWD_V,
                COLOR_F,
                &[
                    Stream { stride: 12, instanced: false, attrs: &fa },
                    Stream { stride: 12, instanced: false, attrs: &fb },
                    Stream { stride: 4, instanced: false, attrs: &[("aColor", 0, U8N, 4)] },
                    Stream { stride: 20, instanced: true, attrs: &[("iPos", 0, F32, 3), ("iTurn", 12, S16N, 2), ("iMisc", 16, U8N, 4)] },
                ],
                msaa.gxm(),
            )?;
            // The effects: a template stream and one record per live effect; the first fragment program adds, the second covers.
            let mut fx_progs = Vec::new();
            for (name, body) in FX_V {
                let prog = gpu.program_blends(
                    name,
                    &defines,
                    &format!("{FX_HEAD}{body}"),
                    FX_F,
                    &[
                        Stream { stride: 12, instanced: false, attrs: &[("aA", 0, S8N, 4), ("aB", 4, S8N, 4), ("aC", 8, S8N, 4)] },
                        Stream { stride: 32, instanced: true, attrs: &[("iPosAge", 0, F32, 4), ("iDirA", 16, F32, 4)] },
                    ],
                    msaa.gxm(),
                    [gpu::Blend::Additive, gpu::Blend::Premultiplied],
                )?;
                fx_progs.push(fx::FxProgram::of(prog));
            }
            let fx_progs: [fx::FxProgram; 4] = fx_progs.try_into().map_err(|_| "effect programs".to_string())?;
            let hud_prog = gpu.program("hud", &defines, HUD_V, HUD_F, &Layout { attrs: &[("aPosition", 0, F32, 2), ("aUv", 8, F32, 2), ("aColor", 16, U8N, 4)], stride: 20 }, 0)?;
            let mut vram = Arena::new(Kind::Cdram, 16 * 1024 * 1024);
            let mut targets = Arena::new(Kind::Main, 4 * 1024 * 1024);
            let post = Post::new(&mut gpu, &mut vram, &mut targets, &defines, msaa)?;
            gpu.finish();

            loading(font, &mut dev, &mut frame_no, &["Uploading the field".into()]);
            let per = (scene.super_cell / scene.cell).round().max(1.0) as i32;
            let world = world::World::load(&p, &mut vram, per)?;
            let hud = Hud::load(&p, &mut vram)?;
            loading(font, &mut dev, &mut frame_no, &["Mustering the army".into()]);
            let crowd = Crowd::load(&p, &scene.crowd_reach, scene.crowd_budget)?;
            let figures = Figures::load(&p, &scene)?;
            let effects = fx::Fx::load(&p, &mut vram)?;
            let sim = requiem_sim::worldfile::load(p.section(pack::SIMW)?).map_err(|e| e.to_string())?;
            Ok((meta, scene, gpu, world_prog, world_lit_prog, color_prog, skin_prog, crowd_prog, crowd_far_prog, fx_progs, hud_prog, world, hud, sim, crowd, figures, effects, vram, post, targets))
        })();
        let (meta, scene, gpu, world_prog, world_lit_prog, color_prog, skin_prog, crowd_prog, crowd_far_prog, fx_progs, hud_prog, mut world, mut hud, mut sim, mut crowd, mut figures, mut effects, vram, mut post, _targets) = match loaded {
            Ok(x) => x,
            Err(e) => fail(font, &mut dev, &mut frame_no, e),
        };
        let pack_bytes = bytes.len();
        drop(bytes);
        let load_ms = t_load.elapsed().as_millis() as u64;
        let skin_lit = Lit::of(&skin_prog);
        let crowd_lit = Lit::of(&crowd_prog);
        let crowd_far_lit = Lit::of(&crowd_far_prog);

        let ring_bytes = Figures::frame_bytes() + Crowd::frame_bytes() + fx::Fx::frame_bytes() + Hud::VERTEX_BYTES + 4096;
        let mut ring = match Ring::new(ring_bytes, 2) {
            Ok(r) => r,
            Err(e) => fail(font, &mut dev, &mut frame_no, e),
        };
        let mut fence = Fence::new(0, 2);
        let control = if live { control_watcher() } else { mpsc::channel().1 };
        let mut set = Settings { auto: true, hud: true, stats: live, cull_cw: true, profile: false, lod_near: scene.lod_near, lod_mid: scene.lod_mid, world: true, crowd: true, mage: true, fx: true, crowd_budget: None, crowd_scale: 1.0, far_from: 2, pace: boot["pace"].as_i64().unwrap_or(2) as i32, look: Look::DEFAULT, view: None, cast: Vec::new(), cast_wait: 0 };

        // Sound: the synthesizer renders at 22.05 kHz; the host module doubles it for the port.
        let mut synth = requiem_sim::audio::Synth::new();
        let sound = pocketjs_vita::audio::start(22050);
        let mut pcm = vec![0i16; 4096];

        let ctx = g::vita2d_get_context();
        let mut timing = Timing { ms: [33.3; Timing::N], at: 0, late: 0, frames: 0 };
        let mut last = Instant::now();
        let mut last_vcount = sceDisplayGetVcount();
        let mut prev_buttons = u32::MAX;
        let mut note: (String, f32) = (String::new(), 0.0);
        let (mut sim_ms, mut build_ms, mut draw_ms, mut gpu_ms, mut wait_ms, mut crowd_ms) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let mut wstats = world::Stats::default();
        let mut cstats = crowd::Stats::default();
        let mut fstats = fx::Stats::default();
        let mut mage_tris = 0u32;
        let mut clock_tick = 0u32;
        let mut most = (0u32, 0u32);

        loop {
            // -------------------------------------------------------------- input and simulation
            let pad = input::read();
            let (buttons, action) = dev.menu.input(pad.buttons);
            let pressed = buttons & !prev_buttons;
            prev_buttons = buttons;
            while let Ok(v) = control.try_recv() {
                apply_control(&v, &mut set, &mut sim);
            }
            if pressed & P_SELECT != 0 && frame_no > 30 {
                sim.reset();
                set.auto = false;
                note = ("AGAIN".into(), 1.2);
            }
            if pressed & P_START != 0 {
                set.auto = !set.auto;
                note = (if set.auto { "AUTOPILOT".into() } else { "MANUAL".into() }, 1.5);
            }
            // Any deliberate input takes over from the autopilot.
            if set.auto && pressed & (P_L | P_R | P_CROSS | P_SQUARE | P_TRIANGLE | P_CIRCLE) != 0 && frame_no > 30 {
                set.auto = false;
                note = ("MANUAL".into(), 1.5);
            }

            // One tick per display refresh: two per frame at thirty frames a second, more when a frame is late.
            let vcount = sceDisplayGetVcount();
            let vblanks = (vcount.wrapping_sub(last_vcount)).clamp(1, 4);
            last_vcount = vcount;
            let t0 = Instant::now();
            let mut events = 0u32;
            for _ in 0..vblanks {
                let mut inp = if set.auto { sim.auto_input() } else if dev.menu.visible { Input::default() } else { pad_input(&input::Pad { buttons, ..pad }) };
                if !set.cast.is_empty() {
                    if set.cast_wait == 0 {
                        inp.buttons |= set.cast.pop().unwrap_or(0);
                        set.cast_wait = 14;
                    } else {
                        set.cast_wait -= 1;
                    }
                }
                sim.tick(inp);
                events |= sim.events;
                synth.control(&sim, sim.events);
            }
            sim_ms = sim_ms * 0.9 + t0.elapsed().as_secs_f32() * 100.0;

            if events & ev::READY != 0 {
                note = ("MANA FULL".into(), 1.4);
            }
            if events & ev::UNSEAL != 0 {
                note = ("THE BINDING COMES UNDONE".into(), 2.2);
            }
            if events & ev::FALLEN != 0 {
                note = ("FALLEN".into(), 2.5);
            }
            if events & ev::WON != 0 {
                note = ("EVERY BINDING IS UNDONE.".into(), 6.0);
            }

            // Keep about three output blocks queued (1536 frames at 22.05 kHz, 70 ms), more at a slower pace.
            if sound {
                let queued = 32 * 1024 - pocketjs_vita::audio::free_frames();
                let want = 2304usize.saturating_sub(queued).min(2048);
                if want > 0 {
                    synth.render(&mut pcm[..want * 2], 22050.0);
                    pocketjs_vita::audio::push(&pcm[..want * 2], 2);
                }
            }

            // -------------------------------------------------------------- camera
            let (eye, look, fov) = match set.view {
                Some((pos, target, fov)) => (pos, (target - pos).norm_or(v3(0.0, 0.0, -1.0)), fov),
                None => {
                    let k = sim.cam.shake * 0.22;
                    let t = sim.tick as f32;
                    (sim.cam.pos + v3(sin(t * 1.7) * k, sin(t * 2.3) * k, cos(t * 1.9) * k), sim.cam.look, sim.cam.fov)
                }
            };
            let vp = mat::mul(&mat::perspective(fov, 960.0 / 544.0, scene.clip_near, scene.clip_far), &mat::view(eye, look, 0.0));
            let planes = mat::planes(&vp);

            // -------------------------------------------------------------- moving geometry
            let t1 = Instant::now();
            let slot = (frame_no % 2) as usize;
            let tw = Instant::now();
            // This slot's ring segment was last used two frames ago: its GPU work must be done.
            fence.wait(slot);
            wait_ms = wait_ms * 0.9 + tw.elapsed().as_secs_f32() * 100.0;
            ring.next_frame();
            let fframe = figures.update(&sim, &mut ring, eye);
            let hud_verts = ring.alloc(Hud::VERTEX_BYTES, 16);
            hud.begin(hud_verts.unwrap_or(core::ptr::null_mut()));
            if set.hud {
                note.1 -= vblanks as f32 / 60.0;
                draw_hud(&mut hud, &sim, &note, set.auto);
            }
            if set.stats {
                let line = format!(
                    "{:.1} fps  {:.1} ms (worst {:.1})  late {}  cpu sim {:.1} crowd {:.1} draw {:.1}  {} knights {}k tris {} draws  world {}k/{}",
                    1000.0 / timing.avg().max(0.1),
                    timing.avg(),
                    timing.worst(),
                    timing.late,
                    sim_ms,
                    crowd_ms,
                    draw_ms,
                    cstats.shown,
                    cstats.tris / 1000,
                    cstats.draws,
                    wstats.tris / 1000,
                    wstats.draws
                );
                hud.text(18, 12.0, 20.0, 0.0, rgba(255, 255, 255, 220), &line);
            }
            build_ms = build_ms * 0.9 + t1.elapsed().as_secs_f32() * 100.0;

            // -------------------------------------------------------------- the scene
            let t2 = Instant::now();
            let light = scene.light(1.0);
            let cast = figures::cast_table(&sim, eye);
            let mut scene_error = post.begin_scene(ctx).err();
            figures.draw_sky(ctx, &color_prog, &vp, eye);
            if set.world {
                world_prog.bind(ctx, false);
                gpu::state_opaque(ctx, set.cull_cw);
                g::sceGxmSetFragmentTexture(ctx, 0, &world.atlas.gxm);
                let lit = world::LitWorld { prog: &world_lit_prog, bounds: world_lit_prog.vs.param("uBounds"), cast: world_lit_prog.vs.param("uCast"), table: &cast };
                wstats = world.draw(ctx, &world_prog, &vp, eye, set.lod_near, set.lod_mid, &lit);
            }
            if let Some(f) = &fframe {
                figures.draw_shadows(ctx, &color_prog, &vp, f, scene.fog_density);
            }
            if set.crowd {
                let tc = Instant::now();
                gpu::state_opaque(ctx, set.cull_cw);
                if let Some(b) = set.crowd_budget {
                    crowd.budget = b;
                }
                cstats = crowd.draw(ctx, &sim, &planes, eye, &mut ring, set.crowd_scale, set.far_from, |far| {
                    let (prog, lit) = if far { (&crowd_far_prog, &crowd_far_lit) } else { (&crowd_prog, &crowd_lit) };
                    prog.bind(ctx, false);
                    lit.set(ctx, &vp, &[], &light, &cast, eye, scene.fog_density);
                });
                crowd_ms = crowd_ms * 0.9 + tc.elapsed().as_secs_f32() * 100.0;
                if cstats.shown > most.0 {
                    most = (cstats.shown, cstats.tris);
                }
            }
            if set.mage {
                mage_tris = figures.draw_mage(ctx, &skin_prog, &skin_lit, &vp, &sim, &light, &cast, eye, scene.fog_density, set.cull_cw);
                mage_tris += figures.draw_demon(ctx, &skin_prog, &skin_lit, &vp, &sim, &light, &cast, eye, scene.fog_density, set.cull_cw);
            }
            if set.fx {
                let right = look.cross(V3::UP).norm_or(v3(1.0, 0.0, 0.0));
                fstats = effects.draw(ctx, &fx_progs, &sim, &vp, eye, right, right.cross(look), &mut ring);
            }
            // A heavy strike holds the frame: while it does, the picture hardens, drains and pulls toward its centre.
            let impact = if sim.stop > 0 && sim.stop_len >= 5 { sim.stop as f32 / sim.stop_len as f32 } else { 0.0 };
            let mut look = set.look;
            look.contrast += 0.55 * impact;
            look.saturation -= 0.6 * impact;
            look.bloom_gain += 0.6 * impact;
            look.vignette += 0.25 * impact;
            if scene_error.is_none() {
                scene_error = post.finish_scene(ctx, &look, &vp, eye, scene.sun_dir).err();
            }
            if let Some(e) = scene_error {
                pocketjs_vita::vita_log(format_args!("requiem: {e}"));
            }
            g::vita2d_pool_reset();
            g::vita2d_start_drawing_advanced(core::ptr::null_mut(), 0);
            post.composite(ctx, &look, max(sim.p.hover * 0.6, impact * 0.9) * if set.view.is_some() { 0.0 } else { 1.0 });
            hud.flush(ctx, &hud_prog, figures.quad_ib);
            // vita2d's overlay (the debug menu) expects its own viewport and no depth.
            g::sceGxmSetViewport(ctx, 480.0, 480.0, 272.0, -272.0, 0.5, 0.5);
            gpu::state_overlay(ctx, false);
            dev.overlay();
            g::sceGxmEndScene(ctx, core::ptr::null(), fence.signal(slot));
            draw_ms = draw_ms * 0.9 + t2.elapsed().as_secs_f32() * 100.0;
            if set.profile {
                let tg = Instant::now();
                fence.wait(slot);
                gpu_ms = gpu_ms * 0.9 + tg.elapsed().as_secs_f32() * 100.0;
            }
            g::vita2d_swap_buffers();
            // Hold the pace: a frame is shown for `pace` refreshes.
            while sceDisplayGetVcount().wrapping_sub(last_vcount) < set.pace {
                sceDisplayWaitVblankStart();
            }

            let now = Instant::now();
            let shown = sceDisplayGetVcount().wrapping_sub(last_vcount);
            timing.push((now - last).as_secs_f32() * 1000.0, shown - set.pace + 1);
            last = now;

            // -------------------------------------------------------------- status
            clock_tick += 1;
            if clock_tick % 60 == 0 {
                // The system lowers the clocks after a suspend; set them again.
                if scePowerGetArmClockFrequency() < CLOCKS[0] - 20 {
                    set_clocks();
                }
            }
            if frame_no % 10 == 0 {
                dev.engine = json!({
                    "stage": "running",
                    "pack": {"path": pack_path, "bytes": pack_bytes, "sha256": pack_sha, "name": meta["name"], "seed": meta["seed"], "source": meta["source"], "profile": meta["profile"]},
                    "loadMs": load_ms, "readMs": read_ms,
                    "frameMs": timing.avg(), "worstMs": timing.worst(), "late": timing.late, "frames": timing.frames, "pace": set.pace,
                    "cpuMs": {"sim": sim_ms, "build": build_ms, "draw": draw_ms, "crowd": crowd_ms, "fenceWait": wait_ms},
                    "gpuMs": if set.profile { json!(gpu_ms) } else { Value::Null },
                    "world": {"draws": wstats.draws, "tris": wstats.tris, "near": wstats.near, "mid": wstats.mid, "far": wstats.far, "lit": wstats.lit},
                    "crowd": {"shown": cstats.shown, "draws": cstats.draws, "tris": cstats.tris, "byLod": cstats.by_lod, "pulled": cstats.pulled, "budget": crowd.budget, "free": sim.crowd.free.len(), "standing": sim.crowd.standing(), "mostShown": most.0, "mostTris": most.1},
                    "mage": {"tris": mage_tris},
                    "fx": {"live": fstats.live, "draws": fstats.draws, "tris": fstats.tris},
                    "settings": {"auto": set.auto, "lodNear": set.lod_near, "lodMid": set.lod_mid, "crowdScale": set.crowd_scale, "cullCw": set.cull_cw, "profile": set.profile, "world": set.world, "crowd": set.crowd, "mage": set.mage, "post": {"bloom": set.look.bloom, "rays": set.look.rays, "speed": set.look.speed}},
                    "player": {"pos": [sim.p.pos.x, sim.p.pos.y, sim.p.pos.z], "act": sim.p.act, "move": sim.p.mv, "hp": sim.p.hp, "mana": sim.p.mana, "kos": sim.p.kos, "chain": sim.p.chain, "tick": sim.tick, "stop": sim.stop},
                    "programs": {"compiled": gpu.compiled, "cached": gpu.cached},
                    "msaa": samples,
                    "memory": {"geometry": world.bytes, "crowd": crowd.bytes, "vram": vram.reserved()},
                    "clockMhz": [scePowerGetArmClockFrequency(), scePowerGetGpuClockFrequency()],
                });
            }
            dev.publish(frame_no, "requiem");
            serve(&mut dev, frame_no, action);
            frame_no = frame_no.wrapping_add(1);
        }
    }
}

fn draw_hud(h: &mut Hud, sim: &Sim, note: &(String, f32), auto: bool) {
    let white = rgba(238, 240, 248, 255);
    let dim = rgba(238, 240, 248, 190);
    // Health and mana, bottom left.
    let hp = sim.p.hp / tune::HP_MAX;
    h.text(18, 30.0, 470.0, 0.0, dim, "MAGE");
    h.rect(30.0, 478.0, 264.0, 14.0, rgba(6, 10, 22, 150));
    h.frame(30.0, 478.0, 264.0, 14.0, 1.5, rgba(255, 255, 255, 130));
    h.rect(33.0, 481.0, 258.0 * hp, 8.0, if hp < 0.25 { rgba(255, 110, 80, 255) } else { rgba(226, 236, 248, 255) });
    let mana = sim.p.mana / tune::MANA_MAX;
    h.rect(30.0, 496.0, 264.0, 8.0, rgba(6, 10, 22, 150));
    h.rect(32.0, 498.0, 260.0 * mana, 4.0, if mana >= 1.0 { rgba(255, 226, 150, 255) } else { rgba(120, 176, 255, 255) });
    // The count, top right.
    h.text(44, 930.0, 56.0, 1.0, white, &format!("{}", sim.p.kos));
    h.text(18, 930.0, 80.0, 1.0, dim, &format!("of {}  -  {} stand", sim.goal, sim.crowd.standing()));
    if sim.p.chain >= 3 {
        let a = (smoothstep(tune::CHAIN_HOLD as f32, tune::CHAIN_HOLD as f32 - 40.0, sim.p.chain_t as f32) * 255.0) as u8;
        h.text(44, 930.0, 150.0, 1.0, rgba(255, 236, 190, a), &format!("{}", sim.p.chain));
        h.text(18, 930.0, 172.0, 1.0, rgba(255, 236, 190, a), "HITS");
    }
    if sim.p.act == act::DOWN {
        h.text(26, 480.0, 250.0, 0.5, white, "FALLEN");
    }
    if note.1 > 0.0 {
        let a = (note.1.min(0.3) / 0.3 * 255.0) as u8;
        h.text(26, 480.0, 132.0, 0.5, rgba(238, 240, 248, a), &note.0);
    }
    if auto {
        if sim.tick < 420 {
            let a = (smoothstep(420.0, 300.0, sim.tick as f32) * 255.0) as u8;
            h.text(44, 480.0, 210.0, 0.5, rgba(238, 240, 248, a), "POCKET REQUIEM");
            h.text(18, 480.0, 240.0, 0.5, rgba(238, 240, 248, a), "SQUARE  strike     TRIANGLE  spell     X  evade     CIRCLE  unseal     L  guard     R  hover");
        }
        h.text(18, 480.0, 528.0, 0.5, dim, "AUTOPILOT  -  press a button to take over");
    }
}

/// Answers wired-debug requests at a frame boundary.
unsafe fn serve(dev: &mut dev::Host, frame: u32, action: Action) {
    let mut request = dev.poll();
    let op = request.as_ref().map(|r| r.command.op).or(match action {
        Action::Capture => Some(Op::Capture),
        _ => None,
    });
    match op {
        Some(Op::Status) => request.take().unwrap().finish(Ok(dev.status(frame, "requiem"))),
        Some(Op::Menu) => {
            dev.menu.visible = !dev.menu.visible;
            request.take().unwrap().finish(Ok(json!({"menu": dev.menu.visible})));
        }
        Some(Op::Capture) => {
            if let Some(request) = request.take() {
                let _ = request.reply.try_send(dev.capture(frame));
            } else {
                dev.capture_from_menu(frame);
            }
        }
        Some(Op::Native) => {
            let request = request.take().unwrap();
            g::vita2d_wait_rendering_done();
            let result = dev::exec_native(request.native_path.as_ref().unwrap());
            request.finish(result.map(|_| json!({})));
        }
        Some(Op::Push | Op::Reload | Op::Reset) => {
            if let Some(request) = request.take() {
                request.finish(Err("Pocket Requiem has no JS guest; use native".into()));
            }
        }
        None => {}
    }
}
