//! Plays the autopilot through a stage and prints what happened and what a
//! tick cost. With no argument it uses a flat test stage; otherwise the path
//! of a world file.
//!
//! `cargo run --release -p requiem-sim --bin harness -- [world.rqsw|-] [seconds] [--cohorts N] [--trace] [--wav out.wav]`

use requiem_sim::worldfile;
use requiem_sim::Sim;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let trace = args.iter().any(|a| a == "--trace");
    let skip: Vec<usize> = ["--wav", "--cohorts"].iter().filter_map(|f| args.iter().position(|a| a == f).map(|i| i + 1)).collect();
    let pos: Vec<&String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && !skip.contains(i)).map(|(_, a)| a).collect();
    let mut sim = match pos.first() {
        Some(p) if p.as_str() != "-" => worldfile::load(&std::fs::read(p.as_str()).expect("read world file")).expect("load world"),
        _ => {
            let (field, stage) = worldfile::test_stage(flag("--cohorts").and_then(|s| s.parse().ok()).unwrap_or(12));
            Sim::new(field, stage)
        }
    };
    let seconds: f32 = pos.get(1).and_then(|s| s.parse().ok()).unwrap_or(60.0);
    let ticks = (seconds * 60.0) as u32;
    let wav = flag("--wav");
    let mut synth = requiem_sim::audio::Synth::new();
    let mut pcm: Vec<i16> = Vec::new();
    let mut count = [0u32; 20];
    let (mut max_free, mut stops, mut worst) = (0usize, 0u32, 0.0f32);
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let t0 = std::time::Instant::now();
    for t in 0..ticks {
        let input = sim.auto_input();
        let a = std::time::Instant::now();
        sim.tick(input);
        worst = worst.max(a.elapsed().as_secs_f32() * 1e6);
        if wav.is_some() {
            synth.control(&sim, sim.events);
            let at = pcm.len();
            // 367.5 frames per tick at 22.05 kHz: alternate 367 and 368.
            pcm.resize(at + (367 + (t as usize & 1)) * 2, 0);
            synth.render(&mut pcm[at..], 22050.0);
        }
        max_free = max_free.max(sim.crowd.free.len());
        stops += (sim.stop > 0) as u32;
        for (i, c) in count.iter_mut().enumerate() {
            *c += (sim.events >> i) & 1;
        }
        for v in [sim.p.pos.x, sim.p.pos.z, sim.p.hp, sim.p.kos as f32] {
            hash = (hash ^ v.to_bits() as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
        if trace && t % 30 == 0 {
            println!(
                "{:6} pos {:7.1} {:5.1} {:7.1} act {} mv {:2} t {:3} hp {:4.0} mana {:3.0} kos {:4} chain {:3} free {:4} near {:3} stop {}",
                t, sim.p.pos.x, sim.p.pos.y, sim.p.pos.z, sim.p.act, sim.p.mv, sim.p.t, sim.p.hp, sim.p.mana, sim.p.kos, sim.p.chain, sim.crowd.free.len(), sim.crowd.near, sim.stop
            );
        }
    }
    let us = t0.elapsed().as_secs_f32() * 1e6 / ticks.max(1) as f32;
    println!("ticks {ticks}  {us:.1} us/tick (worst {worst:.0})  state {hash:016x}");
    println!("knights {}  fallen {}  goal {}  won {}  most free {}  frozen ticks {}", sim.crowd.n, sim.p.kos, sim.goal, sim.won, max_free, stops);
    println!("mage hp {:.0}  mana {:.0}  chains {}", sim.p.hp, sim.p.mana, sim.auto.chains);
    let names = ["swing", "hit", "kill", "heavy", "circle", "beam", "lightning", "fire", "bolt", "burst", "evade", "block", "hurt", "gather", "unseal", "fallen", "revive", "won", "ready", "pillar"];
    let line: Vec<String> = names.iter().zip(count).filter(|(_, c)| *c > 0).map(|(n, c)| format!("{n} {c}")).collect();
    println!("events: {}", line.join("  "));
    if let Some(path) = wav {
        let mut out = Vec::with_capacity(44 + pcm.len() * 2);
        let data = (pcm.len() * 2) as u32;
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&22050u32.to_le_bytes());
        out.extend_from_slice(&(22050u32 * 4).to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data.to_le_bytes());
        for s in &pcm {
            out.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(&path, out).expect("write wav");
        println!("wrote {path}");
    }
}
