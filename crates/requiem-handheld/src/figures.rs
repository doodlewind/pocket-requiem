//! What is not the ground, the props or the army, as vertices a device copies
//! or draws in place: the night sky, its stars and its moon, the soft shadows
//! on the ground, and the bone rows of the mage and the demon.

use requiem_sim::crowd::state;
use requiem_sim::math::*;
use requiem_sim::skel::BONES;
use requiem_sim::Sim;

use crate::mat;
use crate::scene::{powf, Scene};

/// Colour, then position: the order the PSP's GE reads a vertex in.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ColorVertex {
    pub color: [u8; 4],
    pub pos: [f32; 3],
}

pub const SKY_SEGS: usize = 20;
pub const SKY_RINGS: usize = 10;
pub const SKY_VERTS: usize = SKY_RINGS * SKY_SEGS;
pub const SKY_INDICES: usize = (SKY_RINGS - 1) * SKY_SEGS * 6;
/// Stars: one quad each, drawn with the quad indices.
pub const STARS: usize = 220;
pub const STAR_VERTS: usize = STARS * 4;
/// The moon: a halo, the disc and five seas, as fans of `MOON_FAN` rim vertices.
pub const MOON_FAN: usize = 24;
pub const MOON_FANS: usize = 7;
pub const MOON_VERTS: usize = MOON_FANS * (MOON_FAN + 1);
pub const MOON_INDICES: usize = MOON_FANS * MOON_FAN * 3;
/// A shadow: a centre and `FAN` rim vertices, dark in the middle and clear at the rim.
pub const FAN: usize = 8;
/// Shadows per frame: hers and those of the knights nearest the eye.
pub const DISCS: usize = 28;
pub const DISC_VERTS: usize = DISCS * (FAN + 1);
pub const DISC_INDICES: usize = DISCS * FAN * 3;

/// The sky: rings from just under the horizon to the zenith, at `radius` around the origin.
pub fn sky(scene: &Scene, radius: f32, verts: &mut [ColorVertex], idx: &mut [u16]) {
    for r in 0..SKY_RINGS {
        let el = min(-0.14 + powf(r as f32 / (SKY_RINGS - 1) as f32, 1.5) * (PI * 0.5 + 0.14), PI * 0.5);
        for s in 0..SKY_SEGS {
            let az = s as f32 / SKY_SEGS as f32 * TAU;
            let d = v3(cos(el) * cos(az), sin(el), cos(el) * sin(az));
            let c = scene.sky_color(d);
            verts[r * SKY_SEGS + s] = ColorVertex { pos: [d.x * radius, d.y * radius, d.z * radius], color: [scene.encode(c[0]), scene.encode(c[1]), scene.encode(c[2]), 255] };
        }
    }
    let mut k = 0;
    for r in 0..SKY_RINGS - 1 {
        for s in 0..SKY_SEGS {
            let (a, b) = ((r * SKY_SEGS + s) as u16, (r * SKY_SEGS + (s + 1) % SKY_SEGS) as u16);
            let (c, d) = (a + SKY_SEGS as u16, b + SKY_SEGS as u16);
            for i in [a, b, d, a, d, c] {
                idx[k] = i;
                k += 1;
            }
        }
    }
}

/// The stars: small quads facing the origin, placed by a fixed hash. `pixel` is the radians one pixel covers.
pub fn stars(radius: f32, pixel: f32, verts: &mut [ColorVertex]) {
    for i in 0..STARS {
        let h = |n: u32| {
            let mut x = (i as u32).wrapping_mul(0x9e37_79b9) ^ n.wrapping_mul(0x85eb_ca6b);
            x ^= x >> 15;
            x = x.wrapping_mul(0x2c1b_3c6d);
            x ^= x >> 12;
            (x & 0xffff) as f32 / 65536.0
        };
        let y = 0.06 + 0.94 * h(1);
        let r = sqrt(1.0 - y * y);
        let a = h(2) * TAU;
        let d = v3(cos(a) * r, y, sin(a) * r);
        let bright = 0.25 + 0.75 * h(3) * h(3) * h(3);
        let side = d.cross(V3::UP).norm_or(v3(1.0, 0.0, 0.0));
        let up = side.cross(d);
        // Half a pixel to a pixel and a quarter to each side.
        let size = radius * pixel * (0.55 + 0.7 * bright);
        let c = d * radius;
        let color = [(190.0 * bright + 40.0) as u8, (215.0 * bright + 40.0) as u8, 255, (255.0 * bright) as u8];
        for (j, (sx, sy)) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].into_iter().enumerate() {
            let q = c + side * (sx * size) + up * (sy * size);
            verts[i * 4 + j] = ColorVertex { pos: [q.x, q.y, q.z], color };
        }
    }
}

/// The moon, drawn large: a halo, the disc, and five seas on it.
pub fn moon(scene: &Scene, radius: f32, verts: &mut [ColorVertex], idx: &mut [u16]) {
    let enc = |c: [f32; 3], a: u8| [scene.encode(c[0]), scene.encode(c[1]), scene.encode(c[2]), a];
    let ax = scene.sun_dir.cross(V3::UP).norm_or(v3(1.0, 0.0, 0.0));
    let ay = ax.cross(scene.sun_dir);
    let r0 = scene.h.moon_radius * radius;
    let m = scene.h.moon;
    let mut fan = |d: usize, c: V3, r: f32, inner: [u8; 4], outer: [u8; 4]| {
        verts[d * (MOON_FAN + 1)] = ColorVertex { pos: [c.x, c.y, c.z], color: inner };
        for t in 0..MOON_FAN {
            let a = t as f32 / MOON_FAN as f32 * TAU;
            let q = c + ax * (cos(a) * r) + ay * (sin(a) * r);
            verts[d * (MOON_FAN + 1) + 1 + t] = ColorVertex { pos: [q.x, q.y, q.z], color: outer };
        }
    };
    let centre = scene.sun_dir * radius;
    let halo = enc([0.2, 0.34, 0.6], 150);
    fan(0, centre, r0 * 3.2, halo, [halo[0], halo[1], halo[2], 0]);
    fan(1, centre, r0, enc(m, 255), enc(m, 255));
    for (i, (x, y, r, k)) in [(-0.3f32, 0.25f32, 0.26f32, 0.86f32), (0.2, 0.3, 0.2, 0.9), (0.05, -0.2, 0.3, 0.88), (-0.4, -0.3, 0.14, 0.9), (0.42, -0.1, 0.12, 0.92)].into_iter().enumerate() {
        let c = enc([m[0] * k * 0.92, m[1] * k * 0.94, m[2] * k], 255);
        fan(2 + i, centre + ax * (x * r0) + ay * (y * r0), r * r0, c, c);
    }
    fan_indices(idx, MOON_FANS, MOON_FAN);
}

/// Indices for `quads` quads of four vertices: 0 1 2, 0 2 3.
pub fn quad_indices(idx: &mut [u16], quads: usize) {
    for q in 0..quads {
        let b = (q * 4) as u16;
        for (j, o) in [0u16, 1, 2, 0, 2, 3].iter().enumerate() {
            idx[q * 6 + j] = b + o;
        }
    }
}

/// Indices for `count` fans of a centre and `fan` rim vertices.
pub fn fan_indices(idx: &mut [u16], count: usize, fan: usize) {
    for d in 0..count {
        let b = (d * (fan + 1)) as u16;
        for t in 0..fan {
            idx[(d * fan + t) * 3] = b;
            idx[(d * fan + t) * 3 + 1] = b + 1 + t as u16;
            idx[(d * fan + t) * 3 + 2] = b + 1 + ((t + 1) % fan) as u16;
        }
    }
}

/// The shadows on the ground: hers, and those of the knights out of formation within `reach` of the eye.
/// Writes `FAN + 1` vertices per shadow and returns how many there are.
pub fn shadows(sim: &Sim, eye: V3, reach: f32, out: &mut [ColorVertex]) -> u32 {
    let mut discs = 0usize;
    let cap = (out.len() / (FAN + 1)).min(DISCS);
    let mut circle = [(0.0f32, 0.0f32); FAN];
    for (t, c) in circle.iter_mut().enumerate() {
        let a = t as f32 / FAN as f32 * TAU;
        *c = (cos(a), sin(a));
    }
    let mut disc = |c: V3, n: V3, rx: f32, rz: f32, yaw: f32, alpha: u8| {
        if discs >= cap || alpha < 6 {
            return;
        }
        let fwd = heading(yaw);
        let ax = (fwd - n * fwd.dot(n)).norm_or(v3(1.0, 0.0, 0.0));
        let ay = n.cross(ax);
        let v = &mut out[discs * (FAN + 1)..(discs + 1) * (FAN + 1)];
        v[0] = ColorVertex { pos: [c.x, c.y, c.z], color: [0, 2, 8, alpha] };
        for (t, (cs, sn)) in circle.iter().enumerate() {
            let q = c + ax * (cs * rz) + ay * (sn * rx);
            v[1 + t] = ColorVertex { pos: [q.x, q.y, q.z], color: [0, 2, 8, 0] };
        }
        discs += 1;
    };
    let p = sim.p.pos;
    let n = sim.field.normal(p.x, p.z);
    disc(v3(p.x, sim.field.height(p.x, p.z) + 0.04, p.z), n, 0.5, 0.56, sim.p.yaw, 150);
    let reach2 = reach * reach;
    for &i in &sim.crowd.free {
        let i = i as usize;
        let (x, z) = (sim.crowd.x[i], sim.crowd.z[i]);
        let d2 = (x - eye.x) * (x - eye.x) + (z - eye.z) * (z - eye.z);
        if d2 > reach2 || sim.crowd.state[i] >= state::GONE {
            continue;
        }
        let y = sim.field.height(x, z);
        // A knight in the air casts a wider, fainter one.
        let lift = max(sim.crowd.y[i] - y, 0.0);
        let lying = matches!(sim.crowd.state[i], state::DOWN | state::DEAD);
        let fade = saturate(1.0 - d2 / reach2) * (1.0 - saturate(lift / 6.0));
        disc(v3(x, y + 0.04, z), V3::UP, if lying { 0.7 } else { 0.62 } + lift * 0.1, if lying { 1.05 } else { 0.68 } + lift * 0.1, sim.crowd.yaw[i], (135.0 * fade) as u8);
    }
    discs as u32
}

/// Three rows per bone of the draw's `bones`, as a skinning program reads them.
pub fn bone_rows(skin: &[M34; BONES], bones: &[u8], out: &mut [f32]) {
    for (slot, &b) in bones.iter().enumerate() {
        let m = &skin[(b as usize).min(BONES - 1)];
        let o = slot * 12;
        out[o..o + 4].copy_from_slice(&[m.r.x.x, m.r.y.x, m.r.z.x, m.t.x]);
        out[o + 4..o + 8].copy_from_slice(&[m.r.x.y, m.r.y.y, m.r.z.y, m.t.y]);
        out[o + 8..o + 12].copy_from_slice(&[m.r.x.z, m.r.y.z, m.r.z.z, m.t.z]);
    }
}

/// Whether the demon is drawn: within sight and inside the view.
pub fn demon_shown(sim: &Sim, planes: &[[f32; 4]; 6], eye: V3, reach: f32) -> bool {
    let (x, z, _) = sim.stage.demon;
    let at = sim.field.point(x, z);
    (at - eye).len() <= reach && mat::visible(planes, &[at.x - 1.5, at.y - 0.5, at.z - 1.5], &[at.x + 1.5, at.y + 2.6, at.z + 1.5])
}
