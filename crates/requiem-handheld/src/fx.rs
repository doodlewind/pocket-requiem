//! Effects, evaluated into vertices on the CPU.
//!
//! The pack's effects are the Vita's: per layer a template of vertices and
//! eight rows of constants, placed by a vertex program from an effect's age
//! (`vita/shaders/fx_*.cg`). The GE has no vertex program and the PICA's has no
//! room for four of these, so here the same formulas run on the CPU, once per
//! frame, and a device draws the result as two batches of textured, coloured
//! triangles: the layers that cover, then the layers that add light.
//!
//! What the formulas share between the vertices of an instance is computed
//! once per instance; what the four corners of a particle or the two edges of
//! a band share, once per particle or per column. A particle outside its life
//! is not emitted. An effect far from the eye draws a share of its particles,
//! and the effects are taken nearest first, so what a full buffer leaves out
//! is the farthest.

use alloc::vec::Vec;
use requiem_pack::{self as pack, FxEffect, FxHeader, FxLayer};
use requiem_sim::fx::{FxList, LIFE};
use requiem_sim::math::*;
use requiem_sim::Sim;

/// Texture coordinates, colour, position: the interface's vertex layout.
pub type FxVertex = crate::hud::HudVertex;

struct Layer {
    program: u32,
    over: bool,
    p: [[f32; 4]; 8],
    /// Twelve numbers per template vertex.
    v: Vec<f32>,
    idx: Vec<u16>,
}

#[derive(Clone, Copy)]
struct Inst {
    pos: V3,
    age: f32,
    dir: V3,
    a: f32,
    /// Squared distance from the eye.
    d2: f32,
}

pub struct Camera {
    pub eye: V3,
    pub right: V3,
    pub up: V3,
    /// Seconds, for what flickers.
    pub time: f32,
}

/// What `build` wrote: indices `0..over` cover, `over..over + add` add light.
#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct Batches {
    pub verts: u32,
    pub over: u32,
    pub add: u32,
    pub live: u32,
}

pub struct Fx {
    effects: Vec<FxEffect>,
    layers: Vec<Layer>,
    order: Vec<(u8, Inst)>,
    /// Effects farther than this are not drawn.
    pub far: f32,
}

struct Out<'a> {
    v: &'a mut [FxVertex],
    i: &'a mut [u16],
    nv: usize,
    ni: usize,
}

impl Out<'_> {
    #[inline]
    fn room(&self, nv: usize, ni: usize) -> bool {
        self.nv + nv <= self.v.len().min(65535) && self.ni + ni <= self.i.len()
    }
    #[inline]
    fn vert(&mut self, pos: V3, color: [u8; 4], uv: [f32; 2]) {
        self.v[self.nv] = FxVertex { uv, color, pos: [pos.x, pos.y, pos.z] };
        self.nv += 1;
    }
    #[inline]
    fn quad(&mut self, a: usize, b: usize, c: usize, d: usize) {
        for k in [a, b, c, a, c, d] {
            self.i[self.ni] = k as u16;
            self.ni += 1;
        }
    }
}

#[inline]
fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// `x^e` for `x` in 0..1: exact for the common exponents, within two hundredths otherwise.
#[inline]
fn pw(x: f32, e: f32) -> f32 {
    if e == 1.0 {
        x
    } else if e == 0.0 {
        1.0
    } else if x <= 0.0 {
        0.0
    } else if e == 2.0 {
        x * x
    } else if e == 0.5 {
        sqrt(x)
    } else {
        // log2 by the exponent bits and a quadratic on the mantissa; 2^y the same way back.
        let bits = x.to_bits();
        let ex = ((bits >> 23) & 255) as i32 - 128;
        let m = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000);
        let y = e * (ex as f32 + (-0.335_828_78 * m + 2.0) * m - 0.658_717_6);
        let fl = floor(y);
        let f = y - fl;
        let k = (fl as i32 + 127).clamp(1, 254) as u32;
        min(f32::from_bits(k << 23) * ((0.337_189_44 * f + 0.657_636_3) * f + 1.001_724_8), 1.0)
    }
}

#[inline]
fn color(a: &[f32; 4], b: &[f32; 4], t: f32, alpha: f32) -> [u8; 4] {
    // In range by the clamp: the checked conversion would test it again.
    let byte = |x: f32| unsafe { (clamp(x, 0.0, 1.0) * 255.0).to_int_unchecked::<i32>() as u8 };
    [byte(mix(a[0], b[0], t)), byte(mix(a[1], b[1], t)), byte(mix(a[2], b[2], t)), byte(mix(a[3], b[3], t) * alpha)]
}

/// The effect's frame: right and up across its direction.
fn frame(f: V3) -> (V3, V3) {
    let r = (f.cross(V3::UP) + v3(0.0001, 0.0, 0.0)).norm();
    (r, r.cross(f))
}

/// Flat on the ground: forward and right.
fn flat_frame(f: V3) -> (V3, V3) {
    let fh = (v3(f.x, 0.0, f.z) + v3(0.0, 0.0, -0.0001)).norm();
    (fh, v3(-fh.z, 0.0, fh.x))
}

impl Fx {
    /// The `FXPK` section.
    pub fn parse(data: &[u8]) -> Result<Fx, &'static str> {
        let head: FxHeader = pack::read(data, 0).ok_or("effects header")?;
        let mut at = core::mem::size_of::<FxHeader>();
        let mut effects = Vec::with_capacity(head.effects as usize);
        for _ in 0..head.effects {
            effects.push(pack::read::<FxEffect>(data, at).ok_or("effect record")?);
            at += core::mem::size_of::<FxEffect>();
        }
        let mut layers = Vec::with_capacity(head.layers as usize);
        for _ in 0..head.layers {
            let l: FxLayer = pack::read(data, at).ok_or("effect layer")?;
            at += core::mem::size_of::<FxLayer>();
            let (va, ia) = (l.vtx_at as usize, l.idx_at as usize);
            if va + l.vtx_count as usize * 12 > data.len() || ia + l.idx_count as usize * 2 > data.len() || l.program > 3 {
                return Err("the pack's effects section is malformed");
            }
            let v = data[va..va + l.vtx_count as usize * 12].iter().map(|&b| b as i8 as f32 * (1.0 / 127.0)).collect();
            let idx = data[ia..ia + l.idx_count as usize * 2].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            let mut p = [[0.0; 4]; 8];
            for (k, row) in p.iter_mut().enumerate() {
                row.copy_from_slice(&l.rows[k * 4..k * 4 + 4]);
            }
            layers.push(Layer { program: l.program, over: l.blend == 1, p, v, idx });
        }
        if effects.iter().any(|e| (e.first + e.count) as usize > layers.len()) {
            return Err("the pack's effects section is malformed");
        }
        Ok(Fx { effects, layers, order: Vec::with_capacity(192), far: 260.0 })
    }

    /// Writes every live effect's triangles. Indices count from the start of `verts`.
    pub fn build(&mut self, sim: &Sim, cam: &Camera, verts: &mut [FxVertex], idx: &mut [u16]) -> Batches {
        let far2 = self.far * self.far;
        self.order.clear();
        for f in &sim.fx.items {
            if !FxList::alive(f, sim.tick) || f.kind as usize >= self.effects.len() {
                continue;
            }
            let d2 = (f.pos - cam.eye).len2();
            if d2 > far2 {
                continue;
            }
            let age = sim.tick.wrapping_sub(f.t0) as f32 / LIFE[f.kind as usize] as f32;
            self.order.push((f.kind, Inst { pos: f.pos, age, dir: f.dir, a: f.a, d2 }));
        }
        // A bolt in flight is the effect after the simulation's last kind, held at one age, pointing back along its path.
        let bolt = (self.effects.len() - 1) as u8;
        for b in sim.bolts.iter().filter(|b| b.alive != 0) {
            self.order.push((bolt, Inst { pos: b.pos, age: 0.3, dir: -b.vel.norm_or(V3::UP), a: 1.0, d2: (b.pos - cam.eye).len2() }));
        }
        // Nearest first: what the buffers have no room for is the farthest.
        self.order.sort_unstable_by(|a, b| a.1.d2.partial_cmp(&b.1.d2).unwrap_or(core::cmp::Ordering::Equal));
        let mut out = Out { v: verts, i: idx, nv: 0, ni: 0 };
        let mut b = Batches { live: self.order.len() as u32, ..Batches::default() };
        for over in [true, false] {
            for (kind, s) in &self.order {
                let e = self.effects[*kind as usize];
                let (r, u) = frame(s.dir);
                let (fh, rh) = flat_frame(s.dir);
                // Beyond 30 m every second particle, beyond 70 m every fourth.
                let stride = if s.d2 > 70.0 * 70.0 {
                    4
                } else if s.d2 > 30.0 * 30.0 {
                    2
                } else {
                    1
                };
                for l in &self.layers[e.first as usize..(e.first + e.count) as usize] {
                    if l.over != over {
                        continue;
                    }
                    match l.program {
                        0 => particles(l, s, cam, r, u, stride, &mut out),
                        1 => ring(l, s, r, u, fh, rh, &mut out),
                        2 => ribbon(l, s, cam, &mut out),
                        _ => shell(l, s, cam, r, u, fh, rh, &mut out),
                    }
                }
            }
            if over {
                b.over = out.ni as u32;
            }
        }
        b.add = out.ni as u32 - b.over;
        b.verts = out.nv as u32;
        b
    }
}

fn particles(l: &Layer, s: &Inst, cam: &Camera, r: V3, u: V3, stride: usize, out: &mut Out) {
    let p = &l.p;
    let a = mix(1.0, s.a, p[6][0]);
    let stretch = p[2][3];
    let (uv0, uv1) = ([p[5][0], p[5][1]], [p[5][2], p[5][3]]);
    // Four template vertices per particle; the first holds what they share.
    for q in l.v.chunks_exact(48).step_by(stride) {
        let tsec = (s.age - q[4] * p[1][0]) * p[1][3];
        let life = mix(p[1][1], p[1][2], q[5]);
        let t = tsec / life;
        if !(0.0..=1.0).contains(&t) {
            continue;
        }
        if !out.room(4, 6) {
            return;
        }
        let v0 = mix(p[0][0], p[0][1], q[3]) * a;
        let travel = p[6][1] * a + v0 * tsec * (1.0 - 0.5 * p[0][3] * t);
        let d = r * q[0] + u * q[1] + s.dir * q[2];
        let mut world = s.pos + d * travel;
        world.y += (p[6][2] - 0.5 * p[0][2] * tsec) * tsec;
        let size = mix(p[2][0], p[2][1], t) * (1.0 - p[2][2] * q[6]) * a;
        let (ax, ay) = if stretch > 0.0005 {
            let vel = d * (v0 * (1.0 - p[0][3] * t)) + v3(0.0, p[6][2] - p[0][2] * tsec, 0.0);
            let sp = vel.len();
            let axis = vel * (1.0 / max(sp, 0.001));
            let side = (axis.cross(cam.eye - world) + v3(0.0, 0.0001, 0.0)).norm();
            (side * size, axis * (size * (1.0 + stretch * sp)))
        } else {
            (cam.right * size, cam.up * size)
        };
        let alpha = min(t / max(p[7][0], 0.001), 1.0) * pw(1.0 - t, p[6][3]);
        let c = color(&p[3], &p[4], t, alpha);
        if c[3] == 0 {
            continue;
        }
        let first = out.nv;
        out.vert(world - ax - ay, c, [uv0[0], uv0[1]]);
        out.vert(world + ax - ay, c, [uv1[0], uv0[1]]);
        out.vert(world + ax + ay, c, [uv1[0], uv1[1]]);
        out.vert(world - ax + ay, c, [uv0[0], uv1[1]]);
        out.quad(first, first + 1, first + 2, first + 3);
    }
}

#[allow(clippy::too_many_arguments)]
fn ring(l: &Layer, s: &Inst, r: V3, u: V3, fh: V3, rh: V3, out: &mut Out) {
    let p = &l.p;
    let columns = l.v.len() / 24;
    if columns < 2 || !out.room(columns * 2, (columns - 1) * 6) {
        return;
    }
    let a = mix(1.0, abs(s.a), p[2][3]);
    let sgn = mix(1.0, if s.a < 0.0 { -1.0 } else { 1.0 }, p[6][2]);
    let t = s.age;
    let te = pw(t, p[1][3]);
    let (inner, outer) = (mix(p[0][0], p[0][1], te) * a, mix(p[0][2], p[0][3], te) * a);
    let mode = p[6][0];
    let (ax_u, ax_v) = if mode >= 1.5 {
        (fh, rh * cos(p[6][1]) + v3(0.0, sin(p[6][1]) * sgn, 0.0))
    } else if mode >= 0.5 {
        (r, u)
    } else {
        (fh, rh)
    };
    let centre = s.pos + v3(0.0, p[2][2], 0.0);
    let fade = pw(max(1.0 - t, 0.0), p[2][1]);
    let ends = p[7][3];
    let first = out.nv;
    // The columns are evenly spaced along the arc: the first one's angle, then one fixed turn per column
    // (and the same for the fade toward the arc's ends), instead of three trigonometric calls per column.
    let x0 = l.v[0];
    let dx = (l.v[24 * (columns - 1)] - x0) / (columns - 1) as f32;
    let th0 = (x0 * p[1][0] + p[1][1] + p[1][2] * t) * sgn;
    let (mut c, mut s_) = (cos(th0), sin(th0));
    let (cd, sd) = (cos(dx * p[1][0] * sgn), sin(dx * p[1][0] * sgn));
    let e0 = (x0 * 0.5 + 0.5) * PI;
    let (mut ec, mut es) = (cos(e0), sin(e0));
    let (ecd, esd) = (cos(dx * 0.5 * PI), sin(dx * 0.5 * PI));
    let base = color(&p[3], &p[4], t, fade);
    for k in 0..columns {
        let dir = ax_u * c + ax_v * s_;
        let along = (x0 + dx * k as f32) * 0.5 + 0.5;
        let col = if ends > 0.0 { [base[0], base[1], base[2], (base[3] as f32 * mix(1.0, max(es, 0.0), ends)) as u8] } else { base };
        let tu = mix(p[5][0], p[5][2], along);
        out.vert(centre + dir * inner, col, [tu, p[5][1]]);
        out.vert(centre + dir * outer, col, [tu, p[5][3]]);
        (c, s_) = (c * cd - s_ * sd, s_ * cd + c * sd);
        (ec, es) = (ec * ecd - es * esd, es * ecd + ec * esd);
    }
    for k in 0..columns - 1 {
        let b = first + k * 2;
        out.quad(b, b + 2, b + 3, b + 1);
    }
}

fn ribbon(l: &Layer, s: &Inst, cam: &Camera, out: &mut Out) {
    let p = &l.p;
    let a = mix(1.0, s.a, p[1][3]);
    let t = s.age;
    let len = mix(p[0][0], p[0][1], min(t / max(p[2][0], 0.001), 1.0)) * a;
    let width = mix(p[0][2], p[0][3], t) * a;
    let jag = p[1][0] * len;
    let base = if p[6][1] > 0.5 { V3::UP } else { s.dir };
    let centre = s.pos + v3(0.0, p[6][0], 0.0);
    let c = color(&p[3], &p[4], t, pw(max(1.0 - t, 0.0), p[2][1]));
    // A strip is a run of columns (two vertices each) that share the template's `aB`.
    let cols: Vec<&[f32]> = l.v.chunks_exact(24).collect();
    let mut k = 0;
    while k < cols.len() {
        let (side, who) = (cols[k][4], cols[k][5]);
        let mut e = k + 1;
        while e < cols.len() && cols[e][4] == side && cols[e][5] == who {
            e += 1;
        }
        if e - k >= 2 && out.room((e - k) * 2, (e - k - 1) * 6) {
            let fa = side * p[1][2];
            let (fc, fs) = (cos(fa), sin(fa));
            let f = v3(base.x * fc + base.z * fs, base.y, base.z * fc - base.x * fs);
            let (r, u) = frame(f);
            let beat = floor(cam.time * p[1][1] + who * 7.0);
            let first = out.nv;
            for q in &cols[k..e] {
                let x = q[0];
                let mut world = centre + f * (x * len);
                if jag != 0.0 {
                    let w = cos(beat * 2.4 + x * 9.0 + who * 20.0) * jag * sin(x * PI);
                    world += (r * q[2] + u * q[3]) * w;
                }
                let across = (f.cross(world - cam.eye) + v3(0.0, 0.0001, 0.0)).norm();
                let taper = mix(p[2][2], 1.0, smoothstep(0.0, 0.15, x)) * mix(p[2][3], 1.0, smoothstep(1.0, 0.8, x));
                let half = across * (width * taper);
                let tu = mix(p[5][0], p[5][2], x);
                out.vert(world - half, c, [tu, p[5][1]]);
                out.vert(world + half, c, [tu, p[5][3]]);
            }
            for j in 0..e - k - 1 {
                let b = first + j * 2;
                out.quad(b, b + 2, b + 3, b + 1);
            }
        }
        k = e;
    }
}

#[allow(clippy::too_many_arguments)]
fn shell(l: &Layer, s: &Inst, cam: &Camera, r: V3, u: V3, fh: V3, rh: V3, out: &mut Out) {
    let p = &l.p;
    let n = l.v.len() / 12;
    // The template lists every triangle with both windings, for a renderer that culls; drawn unculled,
    // one winding at twice the strength is the same picture at half the fill.
    let tris = l.idx.len() / 12 * 6;
    if !out.room(n, tris) {
        return;
    }
    let a = mix(1.0, s.a, p[2][3]);
    let t = s.age;
    let te = pw(t, p[2][0]);
    let (rad, hgt) = (mix(p[0][0], p[0][1], te) * a, mix(p[0][2], p[0][3], te) * a);
    let ang = p[1][0] + p[1][1] * t;
    let (cs, sn) = (cos(ang), sin(ang));
    let (x, y, z) = if p[6][0] >= 0.5 { (r, s.dir, u) } else { (rh, V3::UP, -fh) };
    let centre = s.pos + y * p[2][2] + s.dir * p[6][1];
    let fade = pw(max(1.0 - t, 0.0), p[2][1]) * 2.0;
    let (rim_power, rim_gain, face) = (p[1][2], p[1][3], p[6][2]);
    let first = out.nv;
    for q in l.v.chunks_exact(12) {
        let (qx, qz) = (q[0] * cs - q[2] * sn, q[0] * sn + q[2] * cs);
        let world = centre + x * (qx * rad) + y * (q[1] * hgt) + z * (qz * rad);
        let mut k = face;
        if rim_gain != 0.0 {
            let nw = x * (q[4] * cs - q[6] * sn) + y * q[5] + z * (q[4] * sn + q[6] * cs);
            k += rim_gain * pw(max(1.0 - abs((cam.eye - world).norm_or(V3::UP).dot(nw)), 0.0), rim_power);
        }
        out.vert(world, color(&p[3], &p[4], t, fade * k), [mix(p[5][0], p[5][2], q[8]), mix(p[5][1], p[5][3], q[9])]);
    }
    for c in l.idx.chunks_exact(12) {
        for &i in &c[..6] {
            out.i[out.ni] = (first + i as usize) as u16;
            out.ni += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fast_power_is_close() {
        for e in [0.3f32, 0.42, 0.9, 1.4, 1.5, 2.4, 2.6, 3.5] {
            for k in 1..=100 {
                let x = k as f32 / 100.0;
                let (got, want) = (pw(x, e), libm::powf(x, e));
                assert!((got - want).abs() < 0.02, "{x}^{e}: {got} vs {want}");
            }
        }
        assert_eq!(pw(0.0, 0.0), 1.0);
        assert_eq!(pw(0.0, 1.3), 0.0);
        assert_eq!(pw(0.25, 0.5), 0.5);
    }
}
