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

use requiem_sim::anim::{solve, Key, Prop};
use requiem_sim::moves::mv;
use requiem_sim::skel::BONES;

/// One arm against the trunk, angles in degrees.
struct Arm {
    /// Where the elbow stands off the line from the shoulder to the wrist, as shares of that
    /// offset in the chest's frame: away from the body, and up the trunk.
    out: f32,
    up: f32,
    /// The upper arm's angle from hanging straight down the trunk.
    from_hanging: f32,
    /// The hand's angle off the line of the forearm.
    wrist: f32,
    /// The wrist is below the shoulder and the elbow stands at least 3 cm off the line.
    hangs: bool,
}

fn arm(w: &[M34; BONES], right: bool) -> Arm {
    let (side, u, l, h) = if right { (1.0, ARM_UR, ARM_LR, HAND_R) } else { (-1.0, ARM_UL, ARM_LL, HAND_L) };
    let c = w[CHEST].r;
    let (s, e, wr) = (w[u].t, w[l].t, w[h].t);
    let axis = (wr - s).norm();
    let off = (e - s) - axis * (e - s).dot(axis);
    let n = off.norm_or(V3::ZERO);
    let angle = |a: V3, b: V3| acos(clamp(a.dot(b), -1.0, 1.0)).to_degrees();
    Arm { out: n.dot(c.x) * side, up: n.dot(c.y), from_hanging: angle((e - s).norm(), -c.y), wrist: angle((wr - e).norm(), -w[h].r.y), hangs: (wr - s).dot(c.y) < 0.0 && off.len() > 0.03 }
}

fn free(speed: f32, phase: f32, hover: f32) -> AnimIn {
    AnimIn { pos: V3::ZERO, yaw: 0.0, speed, show: show::FREE, mv: 0, t: 0.0, phase, hover, tick: (phase * 40.0) as u32, frozen: false }
}

fn shown(what: u8, mv: u8, t: f32) -> AnimIn {
    AnimIn { pos: V3::ZERO, yaw: 0.0, speed: 0.0, show: what, mv, t, phase: 0.0, hover: 0.0, tick: 0, frozen: false }
}

/// Every clip she plays with its length in ticks: the moves, the hit and the fall.
fn clips() -> Vec<(u8, u8, u32)> {
    let lens = [60u32, 30, 30, 36, 40, 54, 46, 44, 58, 66, 78, 24, 156];
    assert_eq!(lens.len(), mv::COUNT);
    let mut all: Vec<(u8, u8, u32)> = lens.iter().enumerate().map(|(m, &n)| (show::MOVE, m as u8, n)).collect();
    all.push((show::HIT, 0, 22));
    all.push((show::DOWN, 0, 40));
    all
}

#[test]
fn the_staff_arm_hangs_while_she_stands_walks_runs_and_hovers() {
    let a = Animator::new();
    for (speed, hover) in [(0.0, 0.0), (0.6, 0.0), (1.1, 0.0), (1.6, 0.0), (3.0, 0.0), (7.0, 0.0), (10.0, 0.5), (14.0, 1.0)] {
        for step in 0..64 {
            let w = solve(&a.skel, &a.want(&free(speed, step as f32 * TAU / 64.0, hover)));
            let r = arm(&w, true);
            let at = format!("speed {speed}, hover {hover}, step {step}");
            assert!(r.hangs, "{at}: her hand is under her shoulder with the elbow bent");
            assert!(r.out < 0.3, "{at}: the elbow stands {:.2} of its offset out from the body", r.out);
            assert!(r.up < 0.25, "{at}: the elbow stands {:.2} of its offset up the trunk", r.up);
            assert!(r.from_hanging < 35.0, "{at}: the upper arm is {:.0} degrees from hanging", r.from_hanging);
            assert!(r.wrist < 30.0, "{at}: the hand is {:.0} degrees off the forearm", r.wrist);
        }
    }
}

#[test]
fn no_clip_carries_the_staff_arms_elbow_out_while_its_hand_is_below_the_shoulder() {
    let a = Animator::new();
    for (what, m, len) in clips() {
        for q in 0..=len * 4 {
            let key = a.want(&shown(what, m, q as f32 / 4.0));
            let r = arm(&solve(&a.skel, &key), true);
            // Fallen, her arms take their angles from the key and follow no staff.
            if r.hangs && key.prop_w > 0.999 {
                assert!(r.out < 0.4, "show {what}, move {m}, tick {}: the elbow stands {:.2} of its offset out from the body", q as f32 / 4.0, r.out);
            }
        }
    }
}

#[test]
fn an_elbow_keeps_pace_with_its_shoulder_and_its_hand() {
    let a = Animator::new();
    for (what, m, len) in clips() {
        let mut prev: Option<[M34; BONES]> = None;
        for q in 0..=len * 4 {
            let w = solve(&a.skel, &a.want(&shown(what, m, q as f32 / 4.0)));
            if let Some(p) = &prev {
                for (u, l, h) in [(ARM_UR, ARM_LR, HAND_R), (ARM_UL, ARM_LL, HAND_L)] {
                    // In a quarter of a tick the elbow goes no more than 8 cm farther than its ends do.
                    let ends = (w[h].t - p[h].t).len().max((w[u].t - p[u].t).len());
                    let own = (w[l].t - p[l].t).len() - ends;
                    assert!(own < 0.08, "show {what}, move {m}, tick {}: an elbow jumps {own:.3} m", q as f32 / 4.0);
                }
            }
            prev = Some(w);
        }
    }
}

#[test]
fn on_guard_each_hand_holds_the_staff_on_its_own_side() {
    let a = Animator::new();
    let w = solve(&a.skel, &a.want(&shown(show::GUARD, 0, 0.0)));
    let (staff, dir) = (w[PROP].t, w[PROP].r.y);
    let left = w[HAND_L].apply(a.skel.palm) - staff;
    assert!((left - dir * left.dot(dir)).len() < 0.002, "the left palm is on the shaft");
    assert!(w[HAND_R].t.x > 0.05 && w[HAND_L].t.x < -0.05, "the hands do not cross");
    for right in [true, false] {
        let r = arm(&w, right);
        assert!(r.out < 0.3 && r.up < 0.0 && r.wrist < 30.0, "an elbow hangs under each hand");
    }
}

#[test]
fn slide_moves_the_hand_along_the_shaft_and_leaves_the_staff_where_it_was() {
    let skel = Animator::new().skel;
    let stand = Key::stand(&skel);
    // The staff upright before her, held at its middle and held 13 cm above it.
    let at = |y: f32, slide: f32| Key { prop: Prop { a: 0.0, r: 0.24, y, az: 0.0, el: PI * 0.5, slide, two: 1.0, off: 0.1, ..stand.prop }, ..stand };
    let (mid, high) = (solve(&skel, &at(0.87, 0.0)), solve(&skel, &at(1.0, 0.13)));
    assert!((mid[PROP].t - high[PROP].t).len() < 0.002, "the staff has not moved");
    assert!((high[HAND_R].t.y - mid[HAND_R].t.y - 0.13).abs() < 0.01, "the right hand is 13 cm higher on it");
    // The left hand keeps its distance below the right one.
    let between = (high[HAND_R].apply(skel.palm) - high[HAND_L].apply(skel.palm)).dot(high[PROP].r.y);
    assert!((between - 0.1).abs() < 0.002, "the left hand is {between:.3} m below the right");
}
