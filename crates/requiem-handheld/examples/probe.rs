//! Runs a handheld pack on the computer the way a device runs it, without a GPU: the autopilot plays, and
//! every frame builds the lists a device draws. Prints what a frame holds at most and on average.
//!
//! `cargo run --release -p requiem-handheld --example probe -- <pack> [seconds]`

use requiem_handheld::crowd::CrowdList;
use requiem_handheld::fx::{Camera, Fx, FxVertex};
use requiem_handheld::game::{Game, Pad};
use requiem_handheld::ground::{Grid, Ground, Layout};
use requiem_handheld::mat;
use requiem_handheld::scene::Scene;
use requiem_handheld::world::World;
use requiem_pack::{self as pack, CrowdHeader, CrowdMesh, HandMesh, HandScene, Pack};
use requiem_sim::math::*;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bytes = std::fs::read(args.first().ok_or("usage: probe <pack> [seconds]")?).map_err(|e| e.to_string())?;
    let seconds: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(90);
    let p = Pack::parse(&bytes)?;
    let hs: HandScene = pack::read(p.section(pack::HSCN)?, 0).ok_or("scene")?;
    let scene = Scene::new(hs);
    let sim = requiem_sim::worldfile::load(p.section(pack::SIMW)?).map_err(|e| e.to_string())?;
    let psp = p.section(pack::CRWP).is_ok();
    let crowd = p.section(if psp { pack::CRWP } else { pack::CRWD })?;
    let head: CrowdHeader = pack::read(crowd, 0).ok_or("crowd")?;
    let tris: Vec<u32> = (0..(head.kinds * head.lods) as usize).map(|i| pack::read::<CrowdMesh>(crowd, 16 + i * core::mem::size_of::<CrowdMesh>()).unwrap().idx_count / 3).collect();
    let mut list = CrowdList::new(&hs, tris, psp);
    let mut fx = Fx::parse(p.section(pack::FXPK)?)?;
    let layout = if psp { Layout::Psp } else { Layout::Pica };
    let mut mem = vec![0u8; Ground::bytes(layout)];
    let mut ground = unsafe { Ground::new(Grid::parse(p.section(pack::GRND)?)?, &sim.field, layout, hs.cell, hs.super_cell, hs.u_range, mem.as_mut_ptr())? };
    let hm = p.section(pack::HMSH)?;
    let recs: Vec<HandMesh> = (0..hm.len() / core::mem::size_of::<HandMesh>()).filter_map(|i| pack::read(hm, i * core::mem::size_of::<HandMesh>())).collect();
    let world = World::new(recs, (hs.super_cell / hs.cell).round() as i32, false, if psp { 12 } else { 16 });
    let mut game = Game::new(sim, scene, "");
    let mut verts = vec![FxVertex::default(); 8192];
    let mut idx = vec![0u16; 16384];
    let (mut far, mut near, mut patches) = (Vec::new(), Vec::new(), Vec::new());
    let frames = seconds * 30;
    let mut max = [0u32; 8];
    let mut sum = [0u64; 8];
    let (mut t_sim, mut t_fx, mut t_crowd, mut t_ground) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for _ in 0..frames {
        let t = std::time::Instant::now();
        game.step(&Pad::default(), 2);
        t_sim += t.elapsed().as_secs_f64();
        let cam = game.camera();
        let vp = game.view_proj(&cam);
        let planes = mat::planes(&vp);
        let t = std::time::Instant::now();
        let cs = list.build(&game.sim, &planes, cam.eye, game.lod_scale);
        t_crowd += t.elapsed().as_secs_f64();
        let right = cam.look.cross(V3::UP).norm_or(v3(1.0, 0.0, 0.0));
        let t = std::time::Instant::now();
        let b = fx.build(&game.sim, &Camera { eye: cam.eye, right, up: right.cross(cam.look), time: game.sim.tick as f32 / 60.0 }, &mut verts, &mut idx);
        t_fx += t.elapsed().as_secs_f64();
        let (ln, lm, lf) = game.lod();
        patches.clear();
        let t = std::time::Instant::now();
        ground.pick(&game.sim.field, &planes, cam.eye, ln, lm, lf, game.frame, &mut patches);
        t_ground += t.elapsed().as_secs_f64();
        far.clear();
        near.clear();
        let ws = world.pick(&planes, cam.eye, ln, lm, lf, &|_| true, &mut far, &mut near);
        let ground_tris: u32 = patches.iter().map(|p| if p.level == 0 { 576 } else { 160 }).sum();
        let prop_tris: u32 = far.iter().chain(&near).map(|p| world.recs[p.mesh as usize].idx_count / 3).sum();
        let row = [cs.shown, cs.tris, b.verts, (b.over + b.add) / 3, b.live, ground_tris, prop_tris, ws.near + ws.mid + ws.far];
        for k in 0..8 {
            max[k] = max[k].max(row[k]);
            sum[k] += row[k] as u64;
        }
    }
    let names = ["knights", "knight triangles", "effect vertices", "effect triangles", "live effects", "ground triangles", "prop triangles", "prop meshes"];
    for k in 0..8 {
        println!("{:>18}: mean {:>7.0}  most {:>6}", names[k], sum[k] as f64 / frames as f64, max[k]);
    }
    let ms = |t: f64| t * 1000.0 / frames as f64;
    println!("this computer, per frame: sim {:.3} ms, crowd list {:.3} ms, effects {:.3} ms, ground {:.3} ms; ground patches built {}", ms(t_sim), ms(t_crowd), ms(t_fx), ms(t_ground), ground.built);
    println!("the autopilot undid {} bindings in {seconds} s; she has {:.0} of her health", game.sim.p.kos, game.sim.p.hp);
    Ok(())
}
