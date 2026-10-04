use requiem_sim::mage::{show, AnimIn, Animator};
use requiem_sim::math::*;
use requiem_sim::skel::bone::*;

#[test]
fn run_pose_keeps_feet_near_the_ground() {
    let mut a = Animator::new();
    let input = AnimIn { pos: V3::ZERO, yaw: 0.0, speed: 7.0, show: show::FREE, mv: 0, t: 0.0, phase: 1.2, hover: 0.0, tick: 0, frozen: false };
    for _ in 0..14 {
        a.update(&input);
    }
    for (name, b) in [("pelvis", PELVIS), ("knee l", LEG_LL), ("foot l", FOOT_L), ("knee r", LEG_LR), ("foot r", FOOT_R), ("head", HEAD)] {
        let p = a.world[b].t;
        println!("{name:8} {:6.3} {:6.3} {:6.3}", p.x, p.y, p.z);
    }
    assert!(a.world[FOOT_L].t.y < 0.2, "the stance foot is on the ground");
    assert!(a.world[PELVIS].t.y > 0.6);
}
