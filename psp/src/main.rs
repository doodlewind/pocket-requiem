//! Pocket Requiem on PSP.
//!
//! The same simulation as the reference and the Vita build (`requiem-sim`),
//! the same stage, lowered by the stage compiler for this machine
//! (`profiles/psp30.json`): 480 × 272 at 30 frames a second, the GE's
//! fixed-function pipeline, 24 MB of memory, one analog stick.
//!
//! Controls: the stick moves, the direction pad turns the camera (left alone,
//! it follows her), square strikes, triangle casts, cross evades, circle
//! undoes the binding when her mana is full, L guards, R hovers. START
//! switches the autopilot, SELECT starts again.
//!
//! Development loop over PSPLINK: the pack is read from `host0:/requiem/`,
//! `control.txt` there steers the run and `status.json` reports it.

#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

mod allocator;
mod audio;
mod gfx;
mod store;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use requiem_handheld::game::{pad, Game, Pad, Perf, Timing};
use requiem_handheld::scene::Scene;
use requiem_handheld::world::World;
use requiem_pack::{self as pack, HandMesh, HandScene, PspVertex};
use requiem_sim::sim::btn;
use psp::sys::*;

psp::module!("PocketRequiem", 1, 0);

const HELP: &str = "SQUARE strike   TRIANGLE spell   X evade   CIRCLE unseal   L guard   R hover";

fn psp_main() {
    psp::enable_home_button();
    unsafe {
        if let Err(e) = run() {
            psp::dprintln!("Could not start: {}", e);
            store::note(&format!("{{\"target\":\"psp\",\"stage\":\"failed\",\"error\":\"{e}\",\"code\":\"{:08x}\"}}", core::ptr::addr_of!(store::LAST_CODE).read() as u32));
            loop {
                sceKernelDelayThread(1_000_000);
            }
        }
    }
}

fn read_pad(data: &SceCtrlData) -> Pad {
    let b = data.buttons;
    let mut buttons = 0;
    for (from, to) in [
        (CtrlButtons::SQUARE, btn::LIGHT),
        (CtrlButtons::TRIANGLE, btn::HEAVY),
        (CtrlButtons::CROSS, btn::EVADE),
        (CtrlButtons::CIRCLE, btn::UNSEAL),
        (CtrlButtons::LTRIGGER, btn::GUARD),
        (CtrlButtons::RTRIGGER, btn::HOVER),
        (CtrlButtons::START, pad::START),
        (CtrlButtons::SELECT, pad::SELECT),
    ] {
        if b.contains(from) {
            buttons |= to;
        }
    }
    // A worn stick rests off centre: nothing inside a quarter of its travel, the rest rescaled.
    let axis = |v: u8| {
        let x = (v as f32 - 127.5) / 127.5;
        let m = if x < 0.0 { -x } else { x };
        if m < 0.24 {
            0.0
        } else {
            (m - 0.24) / 0.76 * if x < 0.0 { -1.0 } else { 1.0 }
        }
    };
    let dir = |neg: CtrlButtons, pos: CtrlButtons| (b.contains(pos) as i32 - b.contains(neg) as i32) as f32 * 0.8;
    Pad { buttons, lx: axis(data.lx), ly: -axis(data.ly), rx: dir(CtrlButtons::LEFT, CtrlButtons::RIGHT), ry: dir(CtrlButtons::DOWN, CtrlButtons::UP) }
}

unsafe fn run() -> Result<(), &'static str> {
    scePowerSetClockFrequency(333, 333, 166);
    psp::dprintln!("Pocket Requiem\n");
    let t_load = sceKernelGetSystemTimeLow();
    let free_at_start = sceKernelTotalFreeMemSize();
    let file = store::PackFile::open()?;
    let mut stage = |name: &str| {
        psp::dprintln!("  {}", name);
        if file.host {
            store::note(&format!("{{\"target\":\"psp\",\"stage\":\"loading\",\"step\":\"{name}\"}}"));
        }
    };

    stage("scene");
    let hs: Vec<HandScene> = file.records(pack::HSCN)?;
    let scene = Scene::new(*hs.first().ok_or("scene section")?);
    let recs: Vec<HandMesh> = file.records(pack::HMSH)?;
    let per = libm::roundf(scene.h.super_cell / scene.h.cell).max(1.0) as i32;
    let world = World::new(recs, per, false, core::mem::size_of::<PspVertex>() as u32);
    stage("stage");
    let sim = {
        let bytes: Vec<u8> = file.records(pack::SIMW)?;
        requiem_sim::worldfile::load(&bytes)?
    };

    let mut gfx = gfx::Gfx::load(&file, &scene, &sim, &mut stage)?;
    // From here the screen belongs to the GE; messages go to the computer only.
    if file.host {
        store::note("{\"target\":\"psp\",\"stage\":\"loading\",\"step\":\"start\"}");
    }
    store::start(&file)?;
    let sound = audio::start();
    let mut game = Game::new(sim, scene, HELP);
    game.set.stats = file.host;
    game.sim.clock = Some(|| unsafe { sceKernelGetSystemTimeLow() });
    game.sim.crowd.clock = game.sim.clock;
    let mut army_part = [0.0f32; 2];
    let mut sim_part = [0.0f32; 4];
    // Test runs: `boot.txt` on the share holds control words for the start, plus `shot=N` (write frame N
    // to `shot.raw`) and `exit=N` (leave at frame N).
    let (mut shot_at, mut exit_at) = (u32::MAX, u32::MAX);
    if file.host {
        let mut buf = [0u8; 512];
        if let Some(text) = store::boot_text(&mut buf) {
            game.control(text);
            for word in text.split_ascii_whitespace() {
                match word.split_once('=') {
                    Some(("shot", n)) => shot_at = requiem_handheld::game::parse_f32(n).unwrap_or(-1.0) as u32,
                    Some(("exit", n)) => exit_at = requiem_handheld::game::parse_f32(n).unwrap_or(-1.0) as u32,
                    _ => {}
                }
            }
        }
    }
    // Which of the two frame buffers the list in flight draws into.
    let mut drawing = 0usize;
    let load_ms = sceKernelGetSystemTimeLow().wrapping_sub(t_load) / 1000;
    let free_after = sceKernelTotalFreeMemSize();

    sceCtrlSetSamplingCycle(0);
    sceCtrlSetSamplingMode(CtrlMode::Analog);
    let mut data: SceCtrlData = core::mem::zeroed();
    let mut pcm = alloc::vec![0i16; 2048];
    let mut status = String::with_capacity(3072);
    let mut extra = String::with_capacity(768);
    let mut timing = Timing::new();
    let mut perf = Perf::default();
    let (mut sim_ms, mut build_ms, mut gpu_ms, mut audio_ms) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut phase_ms = [0.0f32; 7];
    let mut last_swap = sceKernelGetSystemTimeLow();
    let mut last_vcount = sceDisplayGetVcount();
    let mut ticks = game.set.pace;
    let ms = |from: u32| sceKernelGetSystemTimeLow().wrapping_sub(from) as f32 / 1000.0;

    loop {
        // -------------------------------------------------------------- input and simulation
        sceCtrlPeekBufferPositive(&mut data, 1);
        store::control(|text| game.control(text));
        let t0 = sceKernelGetSystemTimeLow();
        game.step(&read_pad(&data), ticks);
        sim_ms = sim_ms * 0.9 + ms(t0) * 0.1;
        for (slot, us) in sim_part.iter_mut().zip(game.sim.prof) {
            *slot = *slot * 0.9 + us as f32 * 0.0001;
        }
        game.sim.prof = [0; 4];
        for (slot, us) in army_part.iter_mut().zip(game.sim.crowd.prof) {
            *slot = *slot * 0.9 + us as f32 * 0.0001;
        }
        game.sim.crowd.prof = [0; 2];
        let ta = sceKernelGetSystemTimeLow();
        if sound && audio::running() {
            let want = audio::wanted();
            if want > 0 {
                game.synth.render(&mut pcm[..want * 2], audio::RATE);
                audio::push(&pcm[..want * 2]);
            }
        }
        audio_ms = audio_ms * 0.9 + ms(ta) * 0.1;
        for (slot, us) in phase_ms.iter_mut().zip(gfx.stats.phase) {
            *slot = *slot * 0.9 + us as f32 * 0.0001;
        }

        // -------------------------------------------------------------- the previous frame leaves the GE
        let t1 = sceKernelGetSystemTimeLow();
        sceGuSync(GuSyncMode::Finish, GuSyncBehavior::Wait);
        gpu_ms = gpu_ms * 0.9 + ms(t1) * 0.1;
        if game.frame == shot_at.wrapping_add(1) {
            // The frame just drawn, out of video memory first: host I/O cannot take a video memory address.
            let vram = sceGeEdramGetAddr().add(drawing * gfx::FB_BYTES);
            let mut pixels = alloc::vec![0u8; 480 * 272 * 2];
            for y in 0..272 {
                core::ptr::copy_nonoverlapping(vram.add(y * 512 * 2), pixels.as_mut_ptr().add(y * 480 * 2), 480 * 2);
            }
            store::write_shot(&pixels);
        }
        if game.frame >= exit_at {
            status.clear();
            extra.clear();
            let _ = write!(extra, "\"exited\":true");
            game.status(&mut status, "psp", &perf, &extra);
            // Let the mailbox thread finish a status write of its own first.
            sceKernelDelayThread(300_000);
            store::note(&status);
            sceKernelExitGame();
        }
        // Present on a display refresh, `pace` of them after the last frame: 30 frames a second.
        let pace = game.set.pace;
        while sceDisplayGetVcount().wrapping_sub(last_vcount) < pace {
            sceDisplayWaitVblankStart();
        }
        sceGuSwapBuffers();
        drawing ^= 1;
        let vcount = sceDisplayGetVcount();
        // One tick per display refresh: a late frame catches up.
        let refreshes = vcount.wrapping_sub(last_vcount);
        ticks = refreshes.clamp(1, 4);
        last_vcount = vcount;
        let now = sceKernelGetSystemTimeLow();
        timing.push(now.wrapping_sub(last_swap) as f32 / 1000.0, refreshes > pace);
        last_swap = now;

        // -------------------------------------------------------------- this frame's list
        let t2 = sceKernelGetSystemTimeLow();
        perf = Perf { frame: timing.avg(), worst: timing.worst(), late: timing.late, frames: timing.frames, sim: sim_ms, build: build_ms, draw: 0.0, gpu: gpu_ms, draws: perf.draws, tris: perf.tris };
        gfx.frame(&mut game, &world, &perf);
        perf.draws = gfx.stats.draws;
        perf.tris = gfx.stats.tris;
        build_ms = build_ms * 0.9 + ms(t2) * 0.1;

        if file.host && game.frame % 15 == 0 {
            status.clear();
            extra.clear();
            let _ = write!(
                extra,
                "\"loadMs\":{},\"pack\":{{\"bytes\":{},\"resident\":{}}},\"memory\":{{\"freeAtStart\":{},\"freeAfterLoad\":{},\"free\":{},\"small\":{},\"large\":{}}},\"clip\":{{\"tested\":{},\"cut\":{}}},\"meshes\":{{\"near\":{},\"mid\":{},\"far\":{}}},\"sound\":{},\"audioMs\":{:.2},\"ticks\":{},\"phaseMs\":{{\"pick\":{:.2},\"figures\":{:.2},\"near\":{:.2},\"far\":{:.2},\"sky\":{:.2},\"effects\":{:.2},\"hud\":{:.2}}},\"pickWorldUs\":{},\"simMs\":{{\"act\":{:.2},\"army\":{:.2},\"camera\":{:.2},\"pose\":{:.2}}},\"armyMs\":{{\"grid\":{:.2},\"loop\":{:.2}}},\"free\":{}",
                load_ms,
                file.bytes,
                gfx.resident_bytes,
                free_at_start,
                free_after,
                sceKernelTotalFreeMemSize(),
                allocator::arena_used(),
                core::ptr::addr_of!(allocator::LARGE_BYTES).read(),
                gfx.stats.tested,
                gfx.stats.clipped,
                gfx.picked.near,
                gfx.picked.mid,
                gfx.picked.far,
                sound && audio::running(),
                audio_ms,
                ticks,
                phase_ms[0],
                phase_ms[1],
                phase_ms[2],
                phase_ms[3],
                phase_ms[4],
                phase_ms[5],
                phase_ms[6],
                gfx.stats.pick_world,
                sim_part[0],
                sim_part[1],
                sim_part[2],
                sim_part[3],
                army_part[0],
                army_part[1],
                game.sim.crowd.free.len()
            );
            game.status(&mut status, "psp", &perf, &extra);
            store::publish(&status);
        }
    }
}
