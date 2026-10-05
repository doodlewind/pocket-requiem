//! The mage stands still and the army closes in: how the simulation's cost grows with the knights out of formation.
//!
//! `cargo run --release -p requiem-handheld --example siege -- <pack> [seconds]`
use requiem_pack::{self as pack, Pack};
use requiem_sim::sim::Input;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bytes = std::fs::read(&args[0]).map_err(|e| e.to_string())?;
    let seconds: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(120);
    let p = Pack::parse(&bytes)?;
    let mut sim = requiem_sim::worldfile::load(p.section(pack::SIMW)?).map_err(|e| e.to_string())?;
    // The autopilot walks her into the army for 20 s; then she stands.
    for second in 0..seconds {
        let t = std::time::Instant::now();
        for _ in 0..60 {
            let input = if second < 20 { sim.auto_input() } else { Input::default() };
            sim.tick(input);
        }
        let us = t.elapsed().as_secs_f64() * 1e6 / 60.0;
        if second % 10 == 9 {
            println!("{:>4} s: {:>4} knights out of formation, {:>6.1} us a tick, {:.3} us a knight; she has {:.0}", second + 1, sim.crowd.free.len(), us, us / sim.crowd.free.len().max(1) as f64, sim.p.hp);
        }
    }
    Ok(())
}
