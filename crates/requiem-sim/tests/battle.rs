use requiem_sim::knight::{self, clip, CLIPS, FRAMES};
use requiem_sim::worldfile::test_stage;
use requiem_sim::Sim;

fn run(ticks: u32) -> (Sim, u64) {
    let (field, stage) = test_stage(12);
    let mut sim = Sim::new(field, stage);
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for _ in 0..ticks {
        let input = sim.auto_input();
        sim.tick(input);
        for v in [sim.p.pos.x, sim.p.pos.z, sim.p.hp, sim.p.kos as f32, sim.crowd.free.len() as f32] {
            hash = (hash ^ v.to_bits() as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    (sim, hash)
}

#[test]
fn the_autopilot_repeats() {
    let (a, ha) = run(1500);
    let (b, hb) = run(1500);
    assert_eq!(ha, hb, "two runs from the same stage diverge");
    assert_eq!(a.p.kos, b.p.kos);
    assert!(a.p.kos > 0, "the autopilot undid no knight in 25 seconds");
}

#[test]
fn a_knight_is_counted_once() {
    let (sim, _) = run(3600);
    assert!(sim.p.kos <= sim.crowd.n as u32, "{} undone of {} knights", sim.p.kos, sim.crowd.n);
    assert_eq!(sim.p.kos, sim.crowd.fallen);
}

#[test]
fn stored_frames_cover_every_clip() {
    let mut total = 0;
    for (c, info) in CLIPS.iter().enumerate() {
        assert_eq!(knight::first(c as u8) as usize, total);
        total += info.frames as usize;
        for k in 0..=40 {
            let u = k as f32 / 40.0 * if info.looped { 3.0 } else { info.len * 1.5 };
            let (a, b, t) = knight::frames(c as u8, u);
            let range = knight::first(c as u8)..knight::first(c as u8) + info.frames;
            assert!(range.contains(&a) && range.contains(&b), "clip {c} at {u} names frames {a} and {b}");
            assert!((0.0..=1.0).contains(&t));
        }
    }
    assert_eq!(total, FRAMES);
    // A frame's time leads back to the same frame.
    for f in 0..FRAMES as u16 {
        let (c, u) = knight::frame_time(f);
        let (a, _, t) = knight::frames(c, u + 1e-4);
        assert_eq!(a, f, "frame {f} of clip {c}");
        assert!(t < 0.05);
    }
    let _ = clip::IDLE;
}
