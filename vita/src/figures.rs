//! What is not the baked ground or the army: the night sky, the mage (skinned
//! on the GPU from the simulation's bone matrices) and the soft shadows on
//! the ground under her and under the knights near her.

use pocket_vita_gxm::mem::{Block, Kind, Ring};
use requiem_pack::{self as pack, ModelHeader, Pack, SkinVertex};
use requiem_sim::crowd::state;
use requiem_sim::fx::{Light, LIGHTS};
use requiem_sim::math::*;
use requiem_sim::skel::BONES;
use requiem_sim::Sim;
use serde_json::Value;
use vita2d_sys as g;

use crate::gpu::{self, Program};
use crate::mat::{self, Mat4};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ColorVertex {
    pub pos: [f32; 3],
    pub color: [u8; 4],
}

pub const QUADS: usize = 2048;
/// A soft disc: a centre and `FAN` rim vertices, opaque in the middle and clear at the rim.
const FAN: usize = 10;
/// Discs per frame: her shadow and the shadows of the knights nearest the eye.
const DISCS: usize = 160;
const SKY_SEGS: usize = 24;
const SKY_RINGS: usize = 13;
const STARS: usize = 420;

/// Scene constants from the pack's META.
pub struct Scene {
    pub sun_dir: V3,
    pub sun: [f32; 3],
    pub sky: [f32; 3],
    pub bounce: [f32; 3],
    pub fog: [f32; 3],
    pub fog_density: f32,
    pub horizon: [f32; 3],
    pub zenith: [f32; 3],
    pub glow: [f32; 3],
    pub moon: [f32; 3],
    pub moon_radius: f32,
    pub lod_near: f32,
    pub lod_mid: f32,
    pub clip_near: f32,
    pub clip_far: f32,
    pub cell: f32,
    pub super_cell: f32,
    pub crowd_reach: Vec<f32>,
    /// Linear 0..2 to sRGB bytes.
    lut: Vec<u8>,
}

fn arr3(v: &Value) -> [f32; 3] {
    let f = |i: usize| v.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    [f(0), f(1), f(2)]
}

impl Scene {
    pub fn from_meta(meta: &Value) -> Scene {
        let s = &meta["scene"];
        let d = arr3(&s["sunDir"]);
        let num = |v: &Value, d: f32| v.as_f64().map(|x| x as f32).unwrap_or(d);
        let lut = (0..1024).map(|i| (libm::powf(i as f32 / 511.5, 1.0 / 2.2).min(1.0) * 255.0 + 0.5) as u8).collect();
        Scene {
            sun_dir: v3(d[0], d[1], d[2]).norm(),
            sun: arr3(&s["sun"]),
            sky: arr3(&s["sky"]),
            bounce: arr3(&s["bounce"]),
            fog: arr3(&s["fog"]),
            fog_density: num(&s["fogDensity"], 0.002),
            horizon: arr3(&s["horizon"]),
            zenith: arr3(&s["zenith"]),
            glow: arr3(&s["glow"]),
            moon: arr3(&s["moon"]),
            moon_radius: num(&s["moonRadius"], 0.1),
            lod_near: num(&s["lod"]["near"], 150.0),
            lod_mid: num(&s["lod"]["mid"], 520.0),
            clip_near: num(&s["clip"]["near"], 0.35),
            clip_far: num(&s["clip"]["far"], 6000.0),
            cell: num(&s["cell"], 64.0),
            super_cell: num(&s["superCell"], 256.0),
            crowd_reach: meta["crowd"]["reach"].as_array().map(|a| a.iter().map(|v| num(v, 100.0)).collect()).unwrap_or_default(),
            lut,
        }
    }

    #[inline]
    pub fn encode(&self, lin: f32) -> u8 {
        self.lut[((lin * 511.5) as usize).min(1023)]
    }

    /// sRGB haze colour, as the shaders take it.
    pub fn fog_srgb(&self) -> [f32; 3] {
        [0, 1, 2].map(|i| libm::powf(self.fog[i], 1.0 / 2.2))
    }

    /// The light table of the lit programs: the moon's direction and how much of it arrives, its colour, sky, bounce.
    pub fn light(&self, vis: f32) -> [f32; 16] {
        [self.sun_dir.x, self.sun_dir.y, self.sun_dir.z, vis, self.sun[0], self.sun[1], self.sun[2], 0.0, self.sky[0], self.sky[1], self.sky[2], 0.0, self.bounce[0], self.bounce[1], self.bounce[2], 0.0]
    }

    /// Sky radiance toward `d` (`web/src/render/sky.ts`).
    pub fn sky_color(&self, d: V3) -> [f32; 3] {
        let h = max(d.y, 0.0);
        let k = 1.0 - libm::powf(1.0 - h, 2.4);
        let s = max(d.dot(self.sun_dir), 0.0);
        let glow = 0.1 * libm::powf(s, 5.0) + 0.5 * libm::powf(s, 60.0);
        let below = saturate(-d.y * 6.0);
        [0, 1, 2].map(|i| {
            let sky = self.horizon[i] + (self.zenith[i] - self.horizon[i]) * k + self.glow[i] * glow;
            sky + (self.fog[i] - sky) * below
        })
    }
}

/// The lights the spells cast, as the lit programs take them: place and 1 / radius², then colour.
pub fn cast_table(sim: &Sim, eye: V3) -> [f32; 32] {
    let mut lights = [Light::default(); LIGHTS];
    let n = sim.fx.lights(sim.tick, eye, &mut lights);
    let mut out = [0.0f32; 32];
    for (k, l) in lights.iter().enumerate().take(n) {
        let o = k * 8;
        out[o..o + 4].copy_from_slice(&[l.pos.x, l.pos.y, l.pos.z, 1.0 / (l.radius * l.radius).max(0.01)]);
        out[o + 4..o + 8].copy_from_slice(&[l.color[0] * l.power, l.color[1] * l.power, l.color[2] * l.power, 0.0]);
    }
    for k in n..LIGHTS {
        out[k * 8 + 3] = 1.0;
    }
    out
}

/// A skinned model in GPU memory.
#[derive(Clone, Copy)]
struct SkinMesh {
    vb: *const u8,
    ib: *const u16,
    idx: u32,
}

/// Uniforms of a lit program.
pub struct Lit {
    mvp: *const g::SceGxmProgramParameter,
    bones: *const g::SceGxmProgramParameter,
    light: *const g::SceGxmProgramParameter,
    cast: *const g::SceGxmProgramParameter,
    eye: *const g::SceGxmProgramParameter,
}

impl Lit {
    pub fn of(prog: &Program) -> Lit {
        Lit { mvp: prog.vs.param("uMvp"), bones: prog.vs.param("uBones"), light: prog.vs.param("uLight"), cast: prog.vs.param("uCast"), eye: prog.vs.param("uEye") }
    }

    /// Reserves the draw's uniforms and writes them. `bones` is empty for a program without any.
    pub unsafe fn set(&self, ctx: *mut g::SceGxmContext, vp: &Mat4, bones: &[f32], light: &[f32; 16], cast: &[f32; 32], eye: V3, fog: f32) {
        let mut buf = core::ptr::null_mut();
        g::sceGxmReserveVertexDefaultUniformBuffer(ctx, &mut buf);
        if buf.is_null() {
            return;
        }
        let put = |p: *const g::SceGxmProgramParameter, v: &[f32]| {
            if !p.is_null() && !v.is_empty() {
                g::sceGxmSetUniformDataF(buf, p, 0, v.len() as u32, v.as_ptr());
            }
        };
        put(self.mvp, vp);
        put(self.bones, bones);
        put(self.light, light);
        put(self.cast, cast);
        put(self.eye, &[eye.x, eye.y, eye.z, fog]);
    }
}

/// Three rows per bone, as the skinning program reads them.
fn bone_rows(skin: &[M34; BONES], out: &mut [f32; BONES * 12]) {
    for (b, m) in skin.iter().enumerate() {
        let o = b * 12;
        out[o..o + 4].copy_from_slice(&[m.r.x.x, m.r.y.x, m.r.z.x, m.t.x]);
        out[o + 4..o + 8].copy_from_slice(&[m.r.x.y, m.r.y.y, m.r.z.y, m.t.y]);
        out[o + 8..o + 12].copy_from_slice(&[m.r.x.z, m.r.y.z, m.r.z.z, m.t.z]);
    }
}

/// What `update` wrote for this frame.
pub struct Frame {
    disc_vb: *const u8,
    discs: u32,
}

pub struct Figures {
    _block: Block,
    sky_vb: *const u8,
    sky_ib: *const u16,
    sky_idx: u32,
    /// The moon: its halo, its disc and its seas, as fans.
    moon_vb: *const u8,
    moon_fans: u32,
    star_vb: *const u8,
    mage: SkinMesh,
    /// Shared indices for quads: 0 1 2, 0 2 3, per four vertices.
    pub quad_ib: *const u16,
    /// Shared indices for discs of `FAN + 1` vertices.
    fan_ib: *const u16,
}

impl Figures {
    /// # Safety
    /// GXM is initialized.
    pub unsafe fn load(p: &Pack, scene: &Scene) -> Result<Figures, String> {
        let modl = p.section(pack::MODL)?;
        let count: u32 = pack::read(modl, 0).ok_or("model count")?;
        let sky_verts = SKY_RINGS * SKY_SEGS;
        let sky_idx = (SKY_RINGS - 1) * SKY_SEGS * 6;
        const MOON_FANS: usize = 7;
        let fans = DISCS.max(MOON_FANS);
        let size = modl.len() + count as usize * 64 + (sky_verts + MOON_FANS * (FAN + 1) + STARS * 4) * 16 + (sky_idx + QUADS * 6 + fans * FAN * 3) * 2 + 1024;
        let mut block = Block::with_access(Kind::Main, size, false)?;
        let mut alloc = |bytes: usize| block.alloc(bytes, 16).ok_or("figure geometry block".to_string());

        // Skinned models, copied as they are in the pack.
        let mut mage = SkinMesh { vb: core::ptr::null(), ib: core::ptr::null(), idx: 0 };
        let mut at = 4;
        for _ in 0..count {
            let h: ModelHeader = pack::read(modl, at).ok_or("model header")?;
            at += core::mem::size_of::<ModelHeader>();
            let vbytes = h.vtx_count as usize * core::mem::size_of::<SkinVertex>();
            let ibytes = h.idx_count as usize * 2;
            if at + vbytes + ibytes > modl.len() {
                return Err("model section is truncated".into());
            }
            let vb = alloc(vbytes)?;
            core::ptr::copy_nonoverlapping(modl.as_ptr().add(at), vb, vbytes);
            at += vbytes;
            let ib = alloc(ibytes)?;
            core::ptr::copy_nonoverlapping(modl.as_ptr().add(at), ib, ibytes);
            at = (at + ibytes + 3) & !3;
            if h.id == 0 {
                mage = SkinMesh { vb, ib: ib.cast(), idx: h.idx_count };
            }
        }
        if mage.idx == 0 {
            return Err("the pack lacks the mage's model".into());
        }

        // Sky: rings from just under the horizon to the zenith.
        let enc = |c: [f32; 3], a: u8| [scene.encode(c[0]), scene.encode(c[1]), scene.encode(c[2]), a];
        let sky_vb = alloc(sky_verts * 16)?.cast::<ColorVertex>();
        let sky_ib = alloc(sky_idx * 2)?.cast::<u16>();
        for r in 0..SKY_RINGS {
            let el = (-0.14 + (r as f32 / (SKY_RINGS - 1) as f32).powf(1.5) * (PI * 0.5 + 0.14)).min(PI * 0.5);
            for s in 0..SKY_SEGS {
                let az = s as f32 / SKY_SEGS as f32 * TAU;
                let d = v3(cos(el) * cos(az), sin(el), cos(el) * sin(az));
                *sky_vb.add(r * SKY_SEGS + s) = ColorVertex { pos: [d.x * 2000.0, d.y * 2000.0, d.z * 2000.0], color: enc(scene.sky_color(d), 255) };
            }
        }
        let mut k = 0;
        for r in 0..SKY_RINGS - 1 {
            for s in 0..SKY_SEGS {
                let (a, b) = ((r * SKY_SEGS + s) as u16, (r * SKY_SEGS + (s + 1) % SKY_SEGS) as u16);
                let (c, d) = (a + SKY_SEGS as u16, b + SKY_SEGS as u16);
                for i in [a, b, d, a, d, c] {
                    *sky_ib.add(k) = i;
                    k += 1;
                }
            }
        }
        // The moon, drawn large: a halo, the disc, and five seas on it.
        let moon_vb = alloc(MOON_FANS * (FAN + 1) * 16)?.cast::<ColorVertex>();
        let ax = scene.sun_dir.cross(V3::UP).norm_or(v3(1.0, 0.0, 0.0));
        let ay = ax.cross(scene.sun_dir);
        let r0 = scene.moon_radius * 1900.0;
        let m = scene.moon;
        let mut fan = |d: usize, c: V3, radius: f32, inner: [u8; 4], outer: [u8; 4]| {
            *moon_vb.add(d * (FAN + 1)) = ColorVertex { pos: [c.x, c.y, c.z], color: inner };
            for t in 0..FAN {
                let a = t as f32 / FAN as f32 * TAU;
                let q = c + ax * (cos(a) * radius) + ay * (sin(a) * radius);
                *moon_vb.add(d * (FAN + 1) + 1 + t) = ColorVertex { pos: [q.x, q.y, q.z], color: outer };
            }
        };
        let centre = scene.sun_dir * 1900.0;
        let halo = enc([0.2, 0.34, 0.6], 150);
        fan(0, centre, r0 * 3.2, halo, [halo[0], halo[1], halo[2], 0]);
        fan(1, centre, r0, enc(m, 255), enc(m, 255));
        for (i, (x, y, r, k)) in [(-0.3f32, 0.25f32, 0.26f32, 0.86f32), (0.2, 0.3, 0.2, 0.9), (0.05, -0.2, 0.3, 0.88), (-0.4, -0.3, 0.14, 0.9), (0.42, -0.1, 0.12, 0.92)].into_iter().enumerate() {
            let c = enc([m[0] * k * 0.92, m[1] * k * 0.94, m[2] * k], 255);
            fan(2 + i, centre + ax * (x * r0) + ay * (y * r0), r * r0, c, c);
        }
        // The stars: small quads facing the eye, placed by a fixed hash.
        let star_vb = alloc(STARS * 4 * 16)?.cast::<ColorVertex>();
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
            let size = 2.6 + 3.4 * bright;
            let c = d * 1900.0;
            let color = [(190.0 * bright + 40.0) as u8, (215.0 * bright + 40.0) as u8, 255, (255.0 * bright) as u8];
            for (j, (sx, sy)) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].into_iter().enumerate() {
                let q = c + side * (sx * size) + up * (sy * size);
                *star_vb.add(i * 4 + j) = ColorVertex { pos: [q.x, q.y, q.z], color };
            }
        }

        let quad_ib = alloc(QUADS * 12)?.cast::<u16>();
        for q in 0..QUADS {
            let b = (q * 4) as u16;
            for (j, o) in [0u16, 1, 2, 0, 2, 3].iter().enumerate() {
                *quad_ib.add(q * 6 + j) = b + o;
            }
        }
        let fan_ib = alloc(fans * FAN * 6)?.cast::<u16>();
        for d in 0..fans {
            let b = (d * (FAN + 1)) as u16;
            for t in 0..FAN {
                *fan_ib.add((d * FAN + t) * 3) = b;
                *fan_ib.add((d * FAN + t) * 3 + 1) = b + 1 + t as u16;
                *fan_ib.add((d * FAN + t) * 3 + 2) = b + 1 + ((t + 1) % FAN) as u16;
            }
        }

        Ok(Figures { _block: block, sky_vb: sky_vb.cast(), sky_ib, sky_idx: sky_idx as u32, moon_vb: moon_vb.cast(), moon_fans: MOON_FANS as u32, star_vb: star_vb.cast(), mage, quad_ib, fan_ib })
    }

    /// Bytes `update` takes from the ring.
    pub fn frame_bytes() -> usize {
        DISCS * (FAN + 1) * 16 + 256
    }

    /// Writes this frame's CPU geometry into the ring: the shadows on the ground.
    ///
    /// # Safety
    /// The ring segment is not in use by the GPU.
    pub unsafe fn update(&mut self, sim: &Sim, ring: &mut Ring, eye: V3) -> Option<Frame> {
        let disc_vb = ring.alloc(DISCS * (FAN + 1) * 16, 16)?.cast::<ColorVertex>();
        let mut discs = 0usize;
        let mut disc = |c: V3, n: V3, rx: f32, rz: f32, yaw: f32, alpha: u8| {
            if discs >= DISCS {
                return;
            }
            let fwd = heading(yaw);
            let ax = (fwd - n * fwd.dot(n)).norm_or(v3(1.0, 0.0, 0.0));
            let ay = n.cross(ax);
            let v = disc_vb.add(discs * (FAN + 1));
            *v = ColorVertex { pos: [c.x, c.y, c.z], color: [0, 2, 8, alpha] };
            for t in 0..FAN {
                let a = t as f32 / FAN as f32 * TAU;
                let q = c + ax * (cos(a) * rz) + ay * (sin(a) * rx);
                *v.add(1 + t) = ColorVertex { pos: [q.x, q.y, q.z], color: [0, 2, 8, 0] };
            }
            discs += 1;
        };
        // The mage's shadow, where the ground is under her.
        let p = sim.p.pos;
        let n = sim.field.normal(p.x, p.z);
        disc(v3(p.x, sim.field.height(p.x, p.z) + 0.03, p.z), n, 0.5, 0.56, sim.p.yaw, 150);
        // The knights out of formation near the eye.
        for &i in &sim.crowd.free {
            let i = i as usize;
            let (x, z) = (sim.crowd.x[i], sim.crowd.z[i]);
            let d2 = (x - eye.x) * (x - eye.x) + (z - eye.z) * (z - eye.z);
            if d2 > 34.0 * 34.0 || sim.crowd.state[i] >= state::GONE {
                continue;
            }
            let y = sim.field.height(x, z);
            // A knight in the air casts a wider, fainter one.
            let lift = max(sim.crowd.y[i] - y, 0.0);
            let lying = matches!(sim.crowd.state[i], state::DOWN | state::DEAD);
            let fade = saturate(1.0 - d2 / (34.0 * 34.0)) * (1.0 - saturate(lift / 6.0));
            disc(v3(x, y + 0.03, z), V3::UP, if lying { 0.7 } else { 0.62 } + lift * 0.1, if lying { 1.05 } else { 0.68 } + lift * 0.1, sim.crowd.yaw[i], (135.0 * fade) as u8);
        }
        Some(Frame { disc_vb: disc_vb.cast(), discs: discs as u32 })
    }

    /// The sky, first in the frame: centred on the eye, no depth, no haze.
    pub unsafe fn draw_sky(&self, ctx: *mut g::SceGxmContext, prog: &Program, vp: &Mat4, eye: V3) {
        prog.bind(ctx, false);
        gpu::state_overlay(ctx, false);
        let m = mat::translated(vp, eye);
        prog.uniforms(ctx, &m, 0.0);
        gpu::draw(ctx, self.sky_vb, self.sky_ib, self.sky_idx);
        prog.bind(ctx, true);
        prog.uniforms(ctx, &m, 0.0);
        gpu::draw(ctx, self.star_vb, self.quad_ib, (STARS * 6) as u32);
        gpu::draw(ctx, self.moon_vb, self.fan_ib, self.moon_fans * (FAN * 3) as u32);
    }

    /// The mage. Returns her triangles.
    pub unsafe fn draw_mage(&self, ctx: *mut g::SceGxmContext, prog: &Program, lit: &Lit, vp: &Mat4, sim: &Sim, light: &[f32; 16], cast: &[f32; 32], eye: V3, fog: f32, cull_cw: bool) -> u32 {
        prog.bind(ctx, false);
        gpu::state_opaque(ctx, cull_cw);
        let mut rows = [0.0f32; BONES * 12];
        bone_rows(&sim.anim.skin, &mut rows);
        lit.set(ctx, vp, &rows, light, cast, eye, fog);
        gpu::draw(ctx, self.mage.vb, self.mage.ib, self.mage.idx);
        self.mage.idx / 3
    }

    /// The shadows, blended over the ground.
    pub unsafe fn draw_shadows(&self, ctx: *mut g::SceGxmContext, prog: &Program, vp: &Mat4, f: &Frame, fog: f32) {
        if f.discs == 0 {
            return;
        }
        prog.bind(ctx, true);
        gpu::state_overlay(ctx, true);
        prog.uniforms(ctx, vp, fog);
        gpu::draw(ctx, f.disc_vb, self.fan_ib, f.discs * (FAN * 3) as u32);
    }
}
