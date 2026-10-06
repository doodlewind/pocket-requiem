//! What is not the baked ground or the army (`vita/src/figures.rs`): the
//! night sky, the mage and the demon (skinned on the GPU from the
//! simulation's bone matrices) and the soft shadows on the ground under her
//! and under the knights near her.

use pocket_web_wgpu::gpu::Gpu;
use pocket_web_wgpu::wgpu;
use requiem_pack::{self as pack, ModelHeader, Pack, SkinVertex};
use requiem_sim::crowd::state;
use requiem_sim::math::*;
use requiem_sim::skel::BONES;
use requiem_sim::Sim;

use super::{buffer, module, part, pipeline, uniform_entry, written, Blend, Depth, Program, Scene, SCENE};
use crate::mat;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ColorVertex {
    pos: [f32; 3],
    color: [u8; 4],
}

/// A soft disc: a centre and `FAN` rim vertices, opaque in the middle and clear at the rim.
const FAN: usize = 10;
/// Discs per frame: her shadow and the shadows of the knights nearest the eye.
const DISCS: usize = 160;
const SKY_SEGS: usize = 24;
const SKY_RINGS: usize = 13;
const STARS: usize = 420;
/// Rim vertices of the moon's fans: it is drawn large, so its edge needs more than a shadow's.
const MOON_FAN: usize = 40;
const MOON_FANS: usize = 7;

/// A skinned model on the GPU, with the buffer its bones' rows are written to.
struct SkinMesh {
    vb: wgpu::Buffer,
    ib: wgpu::Buffer,
    idx: u32,
    rows: wgpu::Buffer,
    group: wgpu::BindGroup,
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

pub struct Figures {
    sky: (wgpu::Buffer, wgpu::Buffer, u32),
    /// The moon: its halo, its disc and its seas, as fans.
    moon: (wgpu::Buffer, wgpu::Buffer, u32),
    stars: (wgpu::Buffer, wgpu::Buffer, u32),
    discs: wgpu::Buffer,
    fans: wgpu::Buffer,
    mage: SkinMesh,
    demon: Option<SkinMesh>,
    dome: wgpu::RenderPipeline,
    lights: wgpu::RenderPipeline,
    shadow: wgpu::RenderPipeline,
    skin: wgpu::RenderPipeline,
    scratch: Vec<ColorVertex>,
    pub bytes: usize,
}

impl Figures {
    pub fn load(gpu: &Gpu, p: &Pack, scene: &Scene, globals: &wgpu::BindGroupLayout, samples: u32) -> Result<Figures, String> {
        let modl = p.section(pack::MODL)?;
        let count: u32 = pack::read(modl, 0).ok_or("model count")?;
        let rows_size = (BONES * 12 * 4) as u64;
        let bones = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("bones"), entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX, rows_size, false)] });

        // Skinned models, as they are in the pack.
        let (mut mage, mut demon) = (None, None);
        let mut at = 4;
        for _ in 0..count {
            let h: ModelHeader = pack::read(modl, at).ok_or("model header")?;
            at += core::mem::size_of::<ModelHeader>();
            let vbytes = h.vtx_count as usize * core::mem::size_of::<SkinVertex>();
            let ibytes = h.idx_count as usize * 2;
            if at + vbytes + ibytes > modl.len() {
                return Err("model section is truncated".into());
            }
            let rows = written(gpu, "bones", rows_size as usize, wgpu::BufferUsages::UNIFORM);
            let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("bones"), layout: &bones, entries: &[wgpu::BindGroupEntry { binding: 0, resource: part(&rows, rows_size) }] });
            let mesh = SkinMesh { vb: buffer(gpu, "model", &modl[at..at + vbytes], wgpu::BufferUsages::VERTEX), ib: buffer(gpu, "model indices", &modl[at + vbytes..at + vbytes + ibytes], wgpu::BufferUsages::INDEX), idx: h.idx_count, rows, group };
            at = (at + vbytes + ibytes + 3) & !3;
            match h.id {
                0 => mage = Some(mesh),
                4 => demon = Some(mesh),
                _ => {}
            }
        }
        let mage = mage.ok_or("the pack lacks the mage's model")?;

        // Sky: rings from just under the horizon to the zenith.
        let enc = |c: [f32; 3], a: u8| [scene.encode(c[0]), scene.encode(c[1]), scene.encode(c[2]), a];
        let mut sky_v = Vec::with_capacity(SKY_RINGS * SKY_SEGS);
        for r in 0..SKY_RINGS {
            let el = (-0.14 + (r as f32 / (SKY_RINGS - 1) as f32).powf(1.5) * (PI * 0.5 + 0.14)).min(PI * 0.5);
            for s in 0..SKY_SEGS {
                let az = s as f32 / SKY_SEGS as f32 * TAU;
                let d = v3(cos(el) * cos(az), sin(el), cos(el) * sin(az));
                sky_v.push(ColorVertex { pos: [d.x * 2000.0, d.y * 2000.0, d.z * 2000.0], color: enc(scene.sky_color(d), 255) });
            }
        }
        let mut sky_i: Vec<u16> = Vec::with_capacity((SKY_RINGS - 1) * SKY_SEGS * 6);
        for r in 0..SKY_RINGS - 1 {
            for s in 0..SKY_SEGS {
                let (a, b) = ((r * SKY_SEGS + s) as u16, (r * SKY_SEGS + (s + 1) % SKY_SEGS) as u16);
                let (c, d) = (a + SKY_SEGS as u16, b + SKY_SEGS as u16);
                sky_i.extend_from_slice(&[a, b, d, a, d, c]);
            }
        }
        // The moon, drawn large: a halo, the disc, and five seas on it.
        let ax = scene.sun_dir.cross(V3::UP).norm_or(v3(1.0, 0.0, 0.0));
        let ay = ax.cross(scene.sun_dir);
        let r0 = scene.moon_radius * 1900.0;
        let m = scene.moon;
        let mut moon_v = Vec::with_capacity(MOON_FANS * (MOON_FAN + 1));
        let mut fan = |c: V3, radius: f32, inner: [u8; 4], outer: [u8; 4]| {
            moon_v.push(ColorVertex { pos: [c.x, c.y, c.z], color: inner });
            for t in 0..MOON_FAN {
                let a = t as f32 / MOON_FAN as f32 * TAU;
                let q = c + ax * (cos(a) * radius) + ay * (sin(a) * radius);
                moon_v.push(ColorVertex { pos: [q.x, q.y, q.z], color: outer });
            }
        };
        let centre = scene.sun_dir * 1900.0;
        let halo = enc([0.2, 0.34, 0.6], 150);
        fan(centre, r0 * 3.2, halo, [halo[0], halo[1], halo[2], 0]);
        fan(centre, r0, enc(m, 255), enc(m, 255));
        for (x, y, r, k) in [(-0.3f32, 0.25f32, 0.26f32, 0.86f32), (0.2, 0.3, 0.2, 0.9), (0.05, -0.2, 0.3, 0.88), (-0.4, -0.3, 0.14, 0.9), (0.42, -0.1, 0.12, 0.92)] {
            let c = enc([m[0] * k * 0.92, m[1] * k * 0.94, m[2] * k], 255);
            fan(centre + ax * (x * r0) + ay * (y * r0), r * r0, c, c);
        }
        let fan_indices = |fans: usize, rim: usize| -> Vec<u16> {
            (0..fans).flat_map(|d| (0..rim).flat_map(move |t| [(d * (rim + 1)) as u16, (d * (rim + 1) + 1 + t) as u16, (d * (rim + 1) + 1 + (t + 1) % rim) as u16])).collect()
        };
        // The stars: small quads facing the eye, placed by a fixed hash.
        let mut star_v = Vec::with_capacity(STARS * 4);
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
            for (sx, sy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let q = c + side * (sx * size) + up * (sy * size);
                star_v.push(ColorVertex { pos: [q.x, q.y, q.z], color });
            }
        }
        let star_i: Vec<u16> = (0..STARS as u16).flat_map(|q| [0u16, 1, 2, 0, 2, 3].map(|o| q * 4 + o)).collect();

        let held = |label: &str, v: &[ColorVertex], i: &[u16]| (buffer(gpu, label, bytemuck::cast_slice(v), wgpu::BufferUsages::VERTEX), buffer(gpu, label, bytemuck::cast_slice(i), wgpu::BufferUsages::INDEX), i.len() as u32);
        let sky = held("sky", &sky_v, &sky_i);
        let moon = held("moon", &moon_v, &fan_indices(MOON_FANS, MOON_FAN));
        let stars = held("stars", &star_v, &star_i);
        let discs = written(gpu, "shadows", DISCS * (FAN + 1) * 16, wgpu::BufferUsages::VERTEX);
        let fans = buffer(gpu, "shadow fans", bytemuck::cast_slice(&fan_indices(DISCS, FAN)), wgpu::BufferUsages::INDEX);

        let color = module(gpu, "color", include_str!("../shaders/color.wgsl"));
        let color_buffers = [wgpu::VertexBufferLayout { array_stride: 16, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4] }];
        let program = |label, vertex, depth, blend| pipeline(gpu, &Program { label, module: &color, vertex, fragment: "tint", groups: &[globals], buffers: &color_buffers, format: SCENE, samples, depth, blend });
        let skin_module = module(gpu, "skin", include_str!("../shaders/skin.wgsl"));
        let skin = pipeline(
            gpu,
            &Program {
                label: "skin",
                module: &skin_module,
                vertex: "skin",
                fragment: "tint",
                groups: &[globals, &bones],
                buffers: &[wgpu::VertexBufferLayout { array_stride: 24, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Snorm8x4, 2 => Unorm8x4, 3 => Uint8x2, 4 => Unorm8x2] }],
                format: SCENE,
                samples,
                depth: Depth::Solid,
                blend: Blend::Opaque,
            },
        );
        Ok(Figures {
            sky,
            moon,
            stars,
            discs,
            fans,
            mage,
            demon,
            dome: program("sky", "sky", Depth::Over, Blend::Opaque),
            lights: program("stars and moon", "sky", Depth::Over, Blend::Alpha),
            shadow: program("shadows", "ground", Depth::Tested, Blend::Alpha),
            skin,
            scratch: Vec::with_capacity(DISCS * (FAN + 1)),
            bytes: modl.len(),
        })
    }

    /// Writes this frame's shadows on the ground and returns how many there are.
    pub fn shadows(&mut self, gpu: &Gpu, sim: &Sim, eye: V3) -> u32 {
        let verts = &mut self.scratch;
        verts.clear();
        let mut disc = |c: V3, n: V3, rx: f32, rz: f32, yaw: f32, alpha: u8| {
            if verts.len() >= DISCS * (FAN + 1) {
                return;
            }
            let fwd = heading(yaw);
            let ax = (fwd - n * fwd.dot(n)).norm_or(v3(1.0, 0.0, 0.0));
            let ay = n.cross(ax);
            verts.push(ColorVertex { pos: [c.x, c.y, c.z], color: [0, 2, 8, alpha] });
            for t in 0..FAN {
                let a = t as f32 / FAN as f32 * TAU;
                let q = c + ax * (cos(a) * rz) + ay * (sin(a) * rx);
                verts.push(ColorVertex { pos: [q.x, q.y, q.z], color: [0, 2, 8, 0] });
            }
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
        gpu.queue.write_buffer(&self.discs, 0, bytemuck::cast_slice(verts));
        (verts.len() / (FAN + 1)) as u32
    }

    /// Writes the bones of the mage, and of the demon when she is within sight. Returns whether she is.
    pub fn pose(&mut self, gpu: &Gpu, sim: &Sim, planes: &[[f32; 4]; 6], eye: V3) -> bool {
        let mut rows = [0.0f32; BONES * 12];
        bone_rows(&sim.anim.skin, &mut rows);
        gpu.queue.write_buffer(&self.mage.rows, 0, bytemuck::cast_slice(&rows));
        let Some(demon) = &self.demon else { return false };
        let (x, z, _) = sim.stage.demon;
        let at = sim.field.point(x, z);
        if (at - eye).len() > 320.0 || !mat::visible(planes, &[at.x - 1.5, at.y - 0.5, at.z - 1.5], &[at.x + 1.5, at.y + 2.6, at.z + 1.5]) {
            return false;
        }
        bone_rows(&requiem_sim::demon::skin(sim), &mut rows);
        gpu.queue.write_buffer(&demon.rows, 0, bytemuck::cast_slice(&rows));
        true
    }

    /// The sky, first in the frame: centred on the eye, no depth, no haze.
    pub fn draw_sky(&self, pass: &mut wgpu::RenderPass) {
        for (program, (vb, ib, count)) in [(&self.dome, &self.sky), (&self.lights, &self.stars), (&self.lights, &self.moon)] {
            pass.set_pipeline(program);
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..*count, 0, 0..1);
        }
    }

    /// The shadows, blended over the ground.
    pub fn draw_shadows(&self, pass: &mut wgpu::RenderPass, discs: u32) {
        if discs == 0 {
            return;
        }
        pass.set_pipeline(&self.shadow);
        pass.set_vertex_buffer(0, self.discs.slice(..));
        pass.set_index_buffer(self.fans.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..discs * (FAN * 3) as u32, 0, 0..1);
    }

    /// The mage, and the demon when `pose` found her within sight. Returns their triangles.
    pub fn draw_figures(&self, pass: &mut wgpu::RenderPass, demon: bool) -> u32 {
        pass.set_pipeline(&self.skin);
        let mut tris = 0;
        for mesh in [Some(&self.mage), self.demon.as_ref().filter(|_| demon)].into_iter().flatten() {
            pass.set_bind_group(1, &mesh.group, &[]);
            pass.set_vertex_buffer(0, mesh.vb.slice(..));
            pass.set_index_buffer(mesh.ib.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..mesh.idx, 0, 0..1);
            tris += mesh.idx / 3;
        }
        tris
    }
}
