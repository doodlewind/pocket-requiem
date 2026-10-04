//! Scene constants of the pack and the lighting rules the reference and the
//! Vita runtime use, for geometry lit on the CPU, by a vertex program or by
//! fixed-function lights.

use alloc::vec::Vec;
use requiem_pack::HandScene;
use requiem_sim::fx::{Light, LIGHTS};
use requiem_sim::math::*;
use requiem_sim::Sim;

pub struct Scene {
    pub h: HandScene,
    /// Toward the moon.
    pub sun_dir: V3,
    /// Linear 0..2 to sRGB bytes.
    lut: Vec<u8>,
}

/// Two colours for a fixed-function pipeline (sRGB-encoded, to multiply an
/// sRGB tint): a constant and the moon.
#[derive(Clone, Copy, Debug)]
pub struct Lights {
    pub ambient: [f32; 3],
    pub moon: [f32; 3],
}

pub fn powf(x: f32, y: f32) -> f32 {
    libm::powf(x, y)
}

fn enc(x: f32) -> f32 {
    powf(max(x, 0.0), 1.0 / 2.2)
}

impl Scene {
    pub fn new(h: HandScene) -> Scene {
        let lut = (0..1024).map(|i| (min(enc(i as f32 / 511.5), 1.0) * 255.0 + 0.5) as u8).collect();
        Scene { sun_dir: v3(h.sun_dir[0], h.sun_dir[1], h.sun_dir[2]).norm(), h, lut }
    }

    #[inline]
    pub fn encode(&self, lin: f32) -> u8 {
        self.lut[((max(lin, 0.0) * 511.5) as usize).min(1023)]
    }

    /// sRGB haze colour.
    pub fn fog_srgb(&self) -> [f32; 3] {
        [0, 1, 2].map(|i| enc(self.h.fog[i]))
    }

    /// The light table of a lit program: the moon's direction and how much of it arrives, its colour, sky, bounce.
    pub fn light(&self, vis: f32) -> [f32; 16] {
        let h = &self.h;
        [self.sun_dir.x, self.sun_dir.y, self.sun_dir.z, vis, h.sun[0], h.sun[1], h.sun[2], 0.0, h.sky[0], h.sky[1], h.sky[2], 0.0, h.bounce[0], h.bounce[1], h.bounce[2], 0.0]
    }

    /// The mage's light as two additive terms (`vita/shaders/skin_v.cg`): exact for a surface the moon
    /// does not reach and for one it reaches in full.
    pub fn figure_lights(&self) -> Lights {
        let mut l = Lights { ambient: [0.0; 3], moon: [0.0; 3] };
        let enc = |x: f32| self.encode(x) as f32 * (1.0 / 255.0);
        for c in 0..3 {
            let shade = self.h.sky[c] * 2.1 + self.h.bounce[c];
            l.ambient[c] = enc(shade);
            l.moon[c] = max(enc(shade + self.h.sun[c] * 1.15) - l.ambient[c], 0.0);
        }
        l
    }

    /// Sky radiance toward `d` (`web/src/render/sky.ts`).
    pub fn sky_color(&self, d: V3) -> [f32; 3] {
        let h = &self.h;
        let up = max(d.y, 0.0);
        let k = 1.0 - powf(1.0 - up, 2.4);
        let s = max(d.dot(self.sun_dir), 0.0);
        let glow = 0.1 * powf(s, 5.0) + 0.5 * powf(s, 60.0);
        let below = saturate(-d.y * 6.0);
        [0, 1, 2].map(|i| {
            let sky = h.horizon[i] + (h.zenith[i] - h.horizon[i]) * k + h.glow[i] * glow;
            sky + (h.fog[i] - sky) * below
        })
    }
}

/// One light a spell casts, as a device takes it.
#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct Cast {
    pub pos: [f32; 3],
    /// 1 / radius².
    pub inv_r2: f32,
    /// Linear colour × power.
    pub color: [f32; 3],
    pub radius: f32,
}

/// The strongest lights the spells cast this frame, strongest first. Returns how many are lit.
pub fn cast(sim: &Sim, eye: V3, out: &mut [Cast; LIGHTS]) -> usize {
    let mut lights = [Light::default(); LIGHTS];
    let n = sim.fx.lights(sim.tick, eye, &mut lights);
    for (k, o) in out.iter_mut().enumerate() {
        *o = if k < n {
            let l = &lights[k];
            Cast { pos: [l.pos.x, l.pos.y, l.pos.z], inv_r2: 1.0 / max(l.radius * l.radius, 0.01), color: [l.color[0] * l.power, l.color[1] * l.power, l.color[2] * l.power], radius: l.radius }
        } else {
            Cast { inv_r2: 1.0, ..Cast::default() }
        };
    }
    n
}
