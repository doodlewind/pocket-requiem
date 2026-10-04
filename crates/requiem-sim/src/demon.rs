//! The demon who holds the army: she stands in front of her ranks with the
//! scales raised in her left hand, and kneels when the count is reached.

use crate::anim::{place, skin as skin_of, solve, Foot, Key};
use crate::math::*;
use crate::sim::Sim;
use crate::skel::{Skeleton, BONES};

/// Skin matrices of the demon at the current tick.
pub fn skin(s: &Sim) -> [M34; BONES] {
    let skel = Skeleton::demon();
    let stand = Key::stand(&skel);
    let t = s.tick as f32 / 60.0;
    let key = if s.won {
        // The scales have tipped: she is on her knees, her arm fallen.
        Key {
            hip: v3(0.0, -0.42, 0.0),
            lean: 0.18,
            bend: 0.35,
            head: [0.0, 0.5],
            prop_w: 0.0,
            arm_r: [0.1, 0.12, 0.2, 0.0],
            arm_l: [0.3, 0.2, 0.4, 0.0],
            feet: [Foot { x: -0.12, fwd: -0.25, lift: 0.02, yaw: -0.1, pitch: -0.8 }, Foot { x: 0.12, fwd: -0.28, lift: 0.02, yaw: 0.1, pitch: -0.8 }],
            ..stand
        }
    } else {
        Key { side: 0.03 * sin(t * 0.8), hip: v3(0.012 * sin(t * 0.8), 0.0, 0.0), head: [0.06 * sin(t * 0.5), -0.06], prop_w: 0.0, arm_r: [0.05, 0.2, 0.35, 0.0], // Raised past the shoulder, a limb's spread turns the other way: negative carries it outward.
        arm_l: [2.15 + 0.04 * sin(t * 1.3), -0.5, 0.55, 0.0], ..stand }
    };
    let mut w = solve(&skel, &key);
    let (x, z, yaw) = s.stage.demon;
    place(&mut w, s.field.point(x, z), yaw);
    skin_of(&w, &skel.bind_inverse())
}
