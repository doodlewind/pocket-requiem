//! Which knights a frame draws, and each one's level of detail.
//!
//! The simulation lists the knights in view with the two stored frames each
//! shows. This keeps the nearest ones the profile's cap and triangle budget pay
//! for, spends the budget on levels of detail from the eye outward, and sorts
//! the result so knights that share a mesh and a frame follow each other: a
//! device binds vertex data once per run.

use alloc::vec::Vec;
use requiem_pack::HandScene;
use requiem_sim::crowd::{Draw, Far};
use requiem_sim::knight;
use requiem_sim::math::*;
use requiem_sim::Sim;

/// One knight to draw.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct Knight {
    pub pos: [f32; 3],
    /// Sine and cosine of its heading.
    pub turn: [f32; 2],
    /// How far from frame `a` to frame `b`.
    pub blend: f32,
    pub scale: f32,
    /// A struck knight flashes: 0..1.
    pub flash: f32,
    pub dist: f32,
    /// Kind × levels + level: the mesh in the pack's table.
    pub mesh: u16,
    pub a: u8,
    pub b: u8,
}

/// Knights a frame may draw as far figures built that frame: those of the cohorts the meshes' reach cuts
/// through, the ones out of formation beyond it, and the ones within it that the meshes' cap left out.
pub const FAR_FIGURES: usize = 448;
/// Vertices of a knight in a cohort's mesh: a quad for the body, across the way the cohort faces, and one
/// for the weapon.
pub const RANK_VERTS: usize = 8;
/// Vertices of one far figure: a body and a weapon, a quad each.
pub const FAR_VERTS: usize = 8;

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct Stats {
    pub shown: u32,
    pub tris: u32,
    pub by_lod: [u32; 4],
    /// The share of the knights shown that are at the level their distance asks for.
    pub pulled: f32,
    /// Knights in view the cap and the budget left out.
    pub dropped: u32,
    /// Knights drawn as far figures.
    pub far: u32,
}

pub struct CrowdList {
    lods: usize,
    reach2: [f32; 8],
    pub far: f32,
    pub budget: u32,
    pub max: usize,
    /// Triangles of each mesh, by kind × levels + level.
    tris: Vec<u32>,
    draws: Vec<Draw>,
    sorted: Vec<Draw>,
    count: Vec<u32>,
    /// Knights within the meshes' reach that the cap left out.
    overflow: Vec<Far>,
    fars: Vec<Far>,
    levels: Vec<u8>,
    pub out: Vec<Knight>,
    /// A machine that blends a stored frame only with the next of its clip (the PSP).
    pairs: bool,
    pub tones: Tones,
}

impl CrowdList {
    /// `tris`: triangles of each mesh in the pack's order. `pairs`: the device stores frames in pairs.
    pub fn new(h: &HandScene, tris: Vec<u32>, pairs: bool) -> CrowdList {
        let lods = (h.crowd_lods as usize).clamp(1, 8);
        let mut reach2 = [0.0; 8];
        for (r2, r) in reach2.iter_mut().zip(h.crowd_reach) {
            *r2 = r * r;
        }
        CrowdList { lods, reach2, far: h.crowd_reach[lods - 1], budget: h.crowd_budget, max: h.crowd_max as usize, tris, draws: Vec::with_capacity(1024), sorted: Vec::with_capacity(1024), count: Vec::with_capacity(512), overflow: Vec::with_capacity(512), fars: Vec::with_capacity(FAR_FIGURES * 2), levels: Vec::with_capacity(512), out: Vec::with_capacity(h.crowd_max as usize), pairs, tones: tones(h, pairs) }
    }

    fn level(&self, d2: f32, k2: f32) -> usize {
        for l in 0..self.lods - 1 {
            if d2 < self.reach2[l] * k2 {
                return l;
            }
        }
        self.lods - 1
    }

    /// Fills `out` for this frame. `scale` pulls every distance in (below 1), for a governor.
    ///
    /// The budget is spent from the eye outward. Every knight starts at the coarsest level, and as many of
    /// the nearest as half the budget pays for are kept; the others are drawn as far figures. A quarter of
    /// the rest buys the finest level for the knights nearest the eye; what remains buys finer levels one
    /// level at a time, nearest first, each knight as far as its distance asks. A press of knights round
    /// the eye then has a fine front rank and no coarse figure near her, and is not late.
    pub fn build(&mut self, sim: &Sim, planes: &[[f32; 4]; 6], eye: V3, scale: f32) -> Stats {
        let mut stats = Stats { pulled: 1.0, ..Stats::default() };
        self.out.clear();
        sim.crowd.draw(&sim.field, sim.tick, planes, eye, self.far * scale, &mut self.draws);
        let low = self.lods - 1;
        let mesh_tris = |tris: &[u32], kind: u8, lod: usize| tris.get((kind as usize).min(2) * self.lods + lod).copied().unwrap_or(0);
        let coarsest = (0..3).map(|k| mesh_tris(&self.tris, k, low)).max().unwrap_or(1).max(1);
        let keep = self.max.min((self.budget as usize / 2) / coarsest as usize);
        // Nearest first, by half-metre steps, and in the simulation's own order inside a step (a counting
        // sort): two knights a hand apart do not trade places from frame to frame, so neither do their levels.
        let steps = (self.far * scale * 2.0) as usize + 2;
        self.count.clear();
        self.count.resize(steps + 1, 0);
        let step_of = |d: &Draw| ((sqrt(d.dist2) * 2.0) as usize).min(steps - 1);
        for d in &self.draws {
            self.count[step_of(d) + 1] += 1;
        }
        for k in 0..steps {
            self.count[k + 1] += self.count[k];
        }
        self.sorted.clear();
        self.sorted.resize(self.draws.len(), Draw { pos: V3::ZERO, yaw: 0.0, a: 0, b: 0, blend: 0.0, scale: 1.0, dist2: 0.0, flash: 0.0, kind: 0, pad: [0; 3] });
        for d in &self.draws {
            let k = step_of(d);
            self.sorted[self.count[k] as usize] = *d;
            self.count[k] += 1;
        }
        core::mem::swap(&mut self.draws, &mut self.sorted);
        // The knights the cap leaves out are still there: on their feet they are drawn as far figures.
        self.overflow.clear();
        if self.draws.len() > keep {
            for d in &self.draws[keep..] {
                if knight::frame_time(d.a).0 <= knight::clip::STAGGER {
                    self.overflow.push(Far { pos: d.pos, dist2: d.dist2, kind: d.kind, big: (d.scale > 1.0) as u8, pad: [0; 2] });
                }
            }
            stats.dropped = (self.draws.len() - keep) as u32;
            self.draws.truncate(keep);
        }
        let k2 = scale * scale;
        let mut left = self.budget as i32 - self.draws.iter().map(|d| mesh_tris(&self.tris, d.kind, low) as i32).sum::<i32>();
        // Level by level, from the coarsest but one to the finest, and nearest first inside a level: every
        // knight near enough for a level gets it before any knight gets a finer one.
        self.levels.clear();
        self.levels.resize(self.draws.len(), low as u8);
        // The front rank first: a quarter of what is left buys the finest level for the knights nearest the eye.
        let front = left / 4;
        let mut spent = 0;
        for (k, d) in self.draws.iter().enumerate() {
            if self.level(d.dist2, k2) != 0 {
                break;
            }
            let cost = mesh_tris(&self.tris, d.kind, 0) as i32 - mesh_tris(&self.tris, d.kind, low) as i32;
            if spent + cost > front {
                break;
            }
            spent += cost;
            self.levels[k] = 0;
        }
        left -= spent;
        for l in (0..low).rev() {
            for (k, d) in self.draws.iter().enumerate() {
                if self.level(d.dist2, k2) > l || self.levels[k] as usize != l + 1 {
                    continue;
                }
                let cost = mesh_tris(&self.tris, d.kind, l) as i32 - mesh_tris(&self.tris, d.kind, l + 1) as i32;
                if cost > left {
                    break;
                }
                left -= cost;
                self.levels[k] = l as u8;
            }
        }
        let mut coarser = 0u32;
        for (k, d) in self.draws.iter().enumerate() {
            let lod = self.levels[k] as usize;
            if lod != self.level(d.dist2, k2) {
                coarser += 1;
            }
            stats.by_lod[lod.min(3)] += 1;
            stats.tris += mesh_tris(&self.tris, d.kind, lod);
            let (mut a, mut b, mut blend) = (d.a as u8, d.b as u8, d.blend);
            if self.pairs && d.b != knight::next(d.a) {
                // Not a stored pair: show the nearer of the two frames.
                if blend >= 0.5 {
                    a = b;
                }
                b = knight::next(a as u16) as u8;
                blend = 0.0;
            }
            self.out.push(Knight { pos: [d.pos.x, d.pos.y, d.pos.z], turn: [sin(d.yaw), cos(d.yaw)], blend, scale: d.scale, flash: d.flash, dist: sqrt(d.dist2), mesh: ((d.kind as usize).min(2) * self.lods + lod) as u16, a, b });
        }
        self.out.sort_unstable_by_key(|k| (k.mesh as u32) << 16 | (k.a as u32) << 8 | k.b as u32);
        stats.shown = self.out.len() as u32;
        if stats.shown > 0 {
            stats.pulled = 1.0 - coarser as f32 / stats.shown as f32;
        }
        stats
    }
}

impl CrowdList {
    /// The knights beyond the meshes' reach that are not part of a cohort drawn whole (`Ranks`), as figures
    /// of two quads facing the eye: a body that widens to the shoulders, moonlit at the top, and the weapon
    /// held upright beside it. `right` is the eye's right; `to` is how far figures are drawn. Writes
    /// `FAR_VERTS` vertices per knight, for the quad indices, and returns the knights written.
    pub fn far_figures(&mut self, sim: &Sim, planes: &[[f32; 4]; 6], eye: V3, right: V3, scale: f32, to: f32, out: &mut [crate::figures::ColorVertex]) -> u32 {
        use crate::figures::ColorVertex;
        let cap = (out.len() / FAR_VERTS).min(FAR_FIGURES);
        sim.crowd.far(&sim.field, planes, eye, self.far * scale, to, self.far * scale, &mut self.fars);
        self.fars.extend_from_slice(&self.overflow);
        if self.fars.len() > cap {
            self.fars.select_nth_unstable_by(cap, |a, b| a.dist2.partial_cmp(&b.dist2).unwrap_or(core::cmp::Ordering::Equal));
            self.fars.truncate(cap);
        }
        let r = v3(right.x, 0.0, right.z).norm_or(v3(1.0, 0.0, 0.0));
        for (k, f) in self.fars.iter().enumerate() {
            let s = if f.big != 0 { 1.2 } else { 1.0 };
            let v = &mut out[k * FAR_VERTS..(k + 1) * FAR_VERTS];
            let at = |x: f32, y: f32, color: [u8; 4]| ColorVertex { color, pos: [f.pos.x + r.x * x * s, f.pos.y + y * s, f.pos.z + r.z * x * s] };
            let [low, high, steel] = self.tones;
            v[0] = at(-0.2, 0.0, low);
            v[1] = at(0.2, 0.0, low);
            v[2] = at(0.34, 1.72, high);
            v[3] = at(-0.34, 1.72, high);
            let tip = TIP[(f.kind as usize).min(2)];
            v[4] = at(0.36, 0.9, low);
            v[5] = at(0.43, 0.9, low);
            v[6] = at(0.43, tip, steel);
            v[7] = at(0.36, tip, steel);
        }
        self.fars.len() as u32
    }
}

/// A far figure's colours: its feet, its shoulders, its weapon's tip.
pub type Tones = [[u8; 4]; 3];

/// The colours of a far figure, from the scene's light as the knights' meshes take it on this machine: the
/// plate's tint in the hemisphere's shade at the feet and under a fifth of the moon at the shoulders (the
/// eye looks toward the moon as often as away from it), and the blade's tint at the tip. `gleam`: the meshes' colours carry the baked gleam (the PSP), so these do too.
pub fn tones(h: &HandScene, gleam: bool) -> Tones {
    const PLATE: [f32; 3] = [0.52, 0.55, 0.62];
    const BLADE: [f32; 3] = [0.68, 0.71, 0.78];
    let enc = |x: f32| libm::powf(max(x, 0.0), 1.0 / 2.2);
    let mut out = [[0u8, 0, 0, 255]; 3];
    for c in 0..3 {
        let hemi = |up: f32| h.bounce[c] + (h.sky[c] - h.bounce[c]) * up;
        let shine = if gleam { enc(h.sun[c] * 0.07 + h.sky[c] * 0.15) * 0.8 } else { 0.0 };
        let (shade, lit) = (enc(hemi(0.5)), enc(h.sun[c] * 0.22 + hemi(0.8)));
        let byte = |x: f32| (min(x, 1.0) * 255.0 + 0.5) as u8;
        out[0][c] = byte(PLATE[c] * shade + shine * 0.6);
        out[1][c] = byte(PLATE[c] * lit + shine);
        out[2][c] = byte(BLADE[c] * lit + shine);
    }
    out
}
/// The weapon's tip, by kind: sword, halberd, greatsword.
const TIP: [f32; 3] = [2.0, 2.9, 2.4];

/// One cohort to draw as a mesh of its ranks.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct RankDraw {
    /// Its first vertex in the ranks' memory, and how many it has (four per quad).
    pub first: u32,
    pub verts: u32,
    /// How far the cohort has marched since the mesh was written: the draw's translation.
    pub offset: [f32; 3],
    /// Written this frame: the device's GPU must be told.
    pub built: u32,
}

struct Rank {
    /// Knights in the mesh (0: not written yet), where the cohort was when it was written, and the
    /// bearing from it to the eye then: the figures face that way.
    knights: u32,
    x: f32,
    z: f32,
    yaw: f32,
    bearing: f32,
}

/// The army beyond the meshes' reach, a cohort at a time.
///
/// At that distance a knight is a dozen pixels tall: a figure of two quads (a body that widens to the
/// shoulders, the weapon held upright) reads as one. A cohort in formation keeps its
/// shape, so its figures are written once, where the cohort stands and facing the eye, and drawn with the
/// cohort's march since then as the draw's translation; they are written again when it has marched 1.5 m,
/// turned, lost a knight, or the eye has moved 17 degrees round it. A frame then draws a thousand knights with a few dozen draws and writes a few dozen.
pub struct Ranks {
    ranks: Vec<Rank>,
    mem: *mut crate::figures::ColorVertex,
    tones: Tones,
    pub draws: Vec<RankDraw>,
    /// Knights in the cohorts drawn this frame.
    pub knights: u32,
}

impl Ranks {
    /// Bytes of vertex memory `new` takes.
    pub fn bytes(sim: &Sim) -> usize {
        sim.crowd.n * RANK_VERTS * core::mem::size_of::<crate::figures::ColorVertex>()
    }

    /// # Safety
    /// `mem` is valid for `bytes(sim)` of memory the GPU reads, and outlives this.
    pub unsafe fn new(sim: &Sim, mem: *mut u8, tones: Tones) -> Ranks {
        Ranks { tones, ranks: sim.crowd.cohorts.iter().map(|c| Rank { knights: 0, x: c.x, z: c.z, yaw: c.yaw, bearing: 0.0 }).collect(), mem: mem.cast(), draws: Vec::with_capacity(sim.crowd.cohorts.len()), knights: 0 }
    }

    /// The memory the draws' `first` counts in.
    pub fn vertices(&self) -> *const crate::figures::ColorVertex {
        self.mem
    }

    /// Fills `draws` with the cohorts wholly between `from` and `to` metres of the eye and in view.
    /// At most `budget` stale meshes are written again; one never written is written regardless.
    pub fn pick(&mut self, sim: &Sim, planes: &[[f32; 4]; 6], eye: V3, from: f32, to: f32, mut budget: u32) {
        use crate::figures::ColorVertex;
        self.draws.clear();
        self.knights = 0;
        let crowd = &sim.crowd;
        for (ci, c) in crowd.cohorts.iter().enumerate() {
            let rank = &mut self.ranks[ci];
            if c.formed == 0 {
                rank.knights = 0;
                continue;
            }
            let centre = v3(c.x, sim.field.height(c.x, c.z) + 1.0, c.z);
            let d = (centre - eye).len();
            if d - c.radius <= from || d - c.radius > to || planes[..4].iter().any(|pl| pl[0] * centre.x + pl[1] * centre.y + pl[2] * centre.z + pl[3] < -(c.radius + 3.0)) {
                continue;
            }
            let (dx, dz) = (c.x - rank.x, c.z - rank.z);
            let bearing = atan2(eye.x - c.x, eye.z - c.z);
            let stale = rank.knights != c.formed || dx * dx + dz * dz > 2.25 || abs(wrap_angle(c.yaw - rank.yaw)) > 0.04 || abs(wrap_angle(bearing - rank.bearing)) > 0.3;
            let mut built = 0;
            if rank.knights == 0 || (stale && budget > 0) {
                budget = budget.saturating_sub(1);
                let (sn, cs) = (sin(c.yaw), cos(c.yaw));
                // Across the line from the cohort to the eye: the figures face the eye as it is now.
                let a = v3(cos(bearing), 0.0, -sin(bearing));
                let mut k = 0usize;
                for i in c.first as usize..(c.first + c.count) as usize {
                    if crowd.state[i] != requiem_sim::crowd::state::FORM {
                        continue;
                    }
                    let (x, z) = (c.x + cs * crowd.sx[i] + sn * crowd.sz[i], c.z - sn * crowd.sx[i] + cs * crowd.sz[i]);
                    let y = sim.field.height(x, z);
                    let s = if crowd.big[i] != 0 { 1.2 } else { 1.0 };
                    let tip = TIP[(crowd.kind[i] as usize).min(2)];
                    let v = unsafe { core::slice::from_raw_parts_mut(self.mem.add(c.first as usize * RANK_VERTS + k * RANK_VERTS), RANK_VERTS) };
                    let at = |along: f32, up: f32, color: [u8; 4]| ColorVertex { color, pos: [x + a.x * along * s, y + up * s, z + a.z * along * s] };
                    let [low, high, steel] = self.tones;
                    v[0] = at(-0.2, 0.0, low);
                    v[1] = at(0.2, 0.0, low);
                    v[2] = at(0.34, 1.72, high);
                    v[3] = at(-0.34, 1.72, high);
                    v[4] = at(0.36, 0.9, low);
                    v[5] = at(0.43, 0.9, low);
                    v[6] = at(0.43, tip, steel);
                    v[7] = at(0.36, tip, steel);
                    k += 1;
                }
                *rank = Rank { knights: k as u32, x: c.x, z: c.z, yaw: c.yaw, bearing };
                built = 1;
            }
            if rank.knights == 0 {
                continue;
            }
            self.knights += rank.knights;
            self.draws.push(RankDraw { first: c.first * RANK_VERTS as u32, verts: rank.knights * RANK_VERTS as u32, offset: [c.x - rank.x, 0.0, c.z - rank.z], built });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> HandScene {
        let mut reach = [0.0; 8];
        reach[..3].copy_from_slice(&[12.0, 40.0, 200.0]);
        HandScene { crowd_lods: 3, crowd_budget: 24_000, crowd_max: 300, crowd_reach: reach, ..Default::default() }
    }

    fn everything() -> [[f32; 4]; 6] {
        [[0.0, 0.0, 0.0, 1.0]; 6]
    }

    #[test]
    fn the_list_keeps_to_its_cap_and_its_budget() {
        let (field, stage) = requiem_sim::worldfile::test_stage(12);
        let sim = Sim::new(field, stage);
        let tris = alloc::vec![700, 114, 50, 700, 122, 50, 730, 114, 50];
        let mut list = CrowdList::new(&scene(), tris.clone(), true);
        let eye = v3(13.0, 2.0, -34.0);
        let s = list.build(&sim, &everything(), eye, 1.0);
        assert!(s.shown > 100 && s.shown <= 300, "{s:?}");
        assert!(s.tris <= 24_000, "{s:?}");
        assert_eq!(s.tris, list.out.iter().map(|k| tris[k.mesh as usize]).sum::<u32>());
        // Sorted, and every pair is a stored one.
        assert!(list.out.windows(2).all(|w| (w[0].mesh, w[0].a) <= (w[1].mesh, w[1].a)));
        assert!(list.out.iter().all(|k| k.b as u16 == knight::next(k.a as u16) && k.blend >= 0.0 && k.blend <= 1.0));
        // A tighter budget shows fewer knights, and never a nearer knight coarser than a farther one of its kind's cost.
        let mut tight = CrowdList::new(&HandScene { crowd_budget: 9_000, ..scene() }, tris, true);
        let t = tight.build(&sim, &everything(), eye, 1.0);
        assert!(t.tris <= 9_000 && t.shown < s.shown && t.shown >= 80, "{t:?}");
        let mut by_dist = tight.out.clone();
        by_dist.sort_by(|a, b| a.dist.partial_cmp(&b.dist).unwrap());
        assert_eq!(by_dist[0].mesh % 3, 0, "the nearest knight is at the finest level");
        assert!(by_dist.windows(2).all(|w| w[0].mesh % 3 <= w[1].mesh % 3), "a nearer knight is coarser than a farther one");
        // Every knight in view is drawn once: as a mesh, as a figure, or in its cohort's ranks.
        let mut mem = alloc::vec![0u8; Ranks::bytes(&sim)];
        let mut ranks = unsafe { Ranks::new(&sim, mem.as_mut_ptr(), list.tones) };
        ranks.pick(&sim, &everything(), eye, list.far, 2000.0, 4);
        let mut figures = alloc::vec![crate::figures::ColorVertex::default(); FAR_FIGURES * FAR_VERTS];
        let far = list.far_figures(&sim, &everything(), eye, v3(1.0, 0.0, 0.0), 1.0, 2000.0, &mut figures);
        assert_eq!(s.shown + far + ranks.knights, sim.crowd.n as u32);
        assert!(ranks.draws.iter().all(|d| d.built == 1 && d.verts % RANK_VERTS as u32 == 0));
        // Nothing moved: the next frame writes nothing.
        ranks.pick(&sim, &everything(), eye, list.far, 2000.0, 4);
        assert!(ranks.draws.iter().all(|d| d.built == 0 && d.offset == [0.0; 3]));
        // With room to spare, the nearest is at the finest level.
        let mut near = list.out.clone();
        near.sort_by(|a, b| a.dist.partial_cmp(&b.dist).unwrap());
        assert_eq!(near[0].mesh % 3, 0);
    }
}
