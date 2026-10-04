//! The ground, built on the device from two grids.
//!
//! The pack holds no ground mesh. The heights are the simulation's grid, which
//! the simulation needs in memory anyway; the baked colours are a second grid
//! of the same size (`GRND`). A frame walks the 256 m super-cells: one beyond
//! the middle distance is one patch of 8 × 8 squares; a closer one draws each
//! of its 64 m cells, 16 × 16 squares inside the near distance and 8 × 8
//! outside it. A patch's vertices are written once, into a slot of the
//! device's vertex memory, and stay until a patch that is needed takes the
//! slot of one no frame has drawn for a while. Every patch of one size shares
//! one index list.
//!
//! A patch has a skirt: its border repeated lower down, so a coarser
//! neighbour leaves no gap.

use alloc::vec::Vec;
use requiem_pack::{self as pack, GroundHeader, PicaVertex, PspVertex};
use requiem_sim::field::Field;
use requiem_sim::math::V3;

use crate::mat;

/// Squares along a side of a near patch, and of a middle or far one.
pub const NEAR_N: usize = 16;
pub const SMALL_N: usize = 8;
pub const NEAR_SLOTS: usize = 28;
pub const SMALL_SLOTS: usize = 112;

pub const fn verts(n: usize) -> usize {
    (n + 1) * (n + 1) + 4 * n
}
pub const fn index_count(n: usize) -> usize {
    n * n * 6 + 4 * n * 6
}

pub mod level {
    pub const NEAR: u8 = 0;
    pub const MID: u8 = 1;
    pub const FAR: u8 = 2;
}
/// Grid squares between a patch's vertices, by level.
const STEP: [usize; 3] = [1, 2, 8];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// `PspVertex`: positions over the patch's bounds.
    Psp,
    /// `PicaVertex`: positions on the world's grid.
    Pica,
}

/// The index list every patch of `n × n` squares shares: the surface, square by square in row order
/// (six indices each), then the skirt.
pub fn indices(n: usize, out: &mut [u16]) {
    let w = n + 1;
    let mut k = 0;
    for b in 0..n {
        for a in 0..n {
            let p = (b * w + a) as u16;
            // The diagonal runs as the simulation reads a square.
            for i in [p, p + w as u16 + 1, p + 1, p, p + w as u16, p + w as u16 + 1] {
                out[k] = i;
                k += 1;
            }
        }
    }
    let ring = ring(n);
    let low = (w * w) as u16;
    for i in 0..ring.len() {
        let j = (i + 1) % ring.len();
        for v in [ring[i], ring[j], low + j as u16, ring[i], low + j as u16, low + i as u16] {
            out[k] = v;
            k += 1;
        }
    }
}

/// The border's vertices, in order around the patch.
fn ring(n: usize) -> Vec<u16> {
    let w = n + 1;
    let mut r = Vec::with_capacity(4 * n);
    r.extend((0..=n).map(|a| a as u16));
    r.extend((1..=n).map(|b| (b * w + n) as u16));
    r.extend((0..n).rev().map(|a| (n * w + a) as u16));
    r.extend((1..n).rev().map(|b| (b * w) as u16));
    r
}

pub struct Grid {
    pub head: GroundHeader,
    colors: Vec<u16>,
}

impl Grid {
    pub fn parse(b: &[u8]) -> Result<Grid, &'static str> {
        let head: GroundHeader = pack::read(b, 0).ok_or("ground header")?;
        let n = head.n as usize;
        let at = core::mem::size_of::<GroundHeader>();
        if b.len() < at + n * n * 2 || n < 2 {
            return Err("the ground section is truncated");
        }
        let colors = b[at..at + n * n * 2].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Ok(Grid { head, colors })
    }
}

/// One patch a frame draws.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Patch {
    /// Where its vertices are: bytes from the start of the vertex memory.
    pub offset: u32,
    pub level: u8,
    /// Its vertices were written this frame: the device's GPU must be told.
    pub built: u8,
    /// Its place in the grid of patches of its level.
    pub cx: u8,
    pub cz: u8,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub dist: f32,
}

#[derive(Clone, Copy)]
struct Slot {
    key: u32,
    used: u32,
}
const NONE: u32 = u32::MAX;

pub struct Ground {
    pub grid: Grid,
    layout: Layout,
    mem: *mut u8,
    vertex_bytes: usize,
    u_range: f32,
    /// Grid squares along a cell and along a super-cell.
    per_cell: usize,
    per_super: usize,
    /// Lowest and highest height of every cell and of every super-cell.
    cell_y: Vec<(f32, f32)>,
    super_y: Vec<(f32, f32)>,
    near: [Slot; NEAR_SLOTS],
    small: [Slot; SMALL_SLOTS],
    pub built: u32,
}

impl Ground {
    /// Bytes of vertex memory `new` takes.
    pub fn bytes(layout: Layout) -> usize {
        let vb = if layout == Layout::Psp { core::mem::size_of::<PspVertex>() } else { core::mem::size_of::<PicaVertex>() };
        (NEAR_SLOTS * verts(NEAR_N) + SMALL_SLOTS * verts(SMALL_N)) * vb
    }

    /// `cell` and `super_cell` are the scene's sizes in metres; `mem` is `bytes(layout)` of memory the GPU reads.
    ///
    /// # Safety
    /// `mem` is valid for `bytes(layout)` and outlives the ground.
    pub unsafe fn new(grid: Grid, field: &Field, layout: Layout, cell: f32, super_cell: f32, u_range: f32, mem: *mut u8) -> Result<Ground, &'static str> {
        let n = field.n;
        let per_cell = libm::roundf(cell / field.cell) as usize;
        let per_super = libm::roundf(super_cell / field.cell) as usize;
        if grid.head.n as usize != n || per_cell != NEAR_N || per_super != SMALL_N * STEP[level::FAR as usize] || (n - 1) % per_super != 0 || (n - 1) / per_cell > 255 {
            return Err("the ground's grid does not match the simulation's");
        }
        let range = |per: usize| -> Vec<(f32, f32)> {
            let side = (n - 1) / per;
            let mut out = Vec::with_capacity(side * side);
            for cz in 0..side {
                for cx in 0..side {
                    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                    for j in cz * per..=(cz + 1) * per {
                        for i in cx * per..=(cx + 1) * per {
                            let h = field.h[j * n + i];
                            lo = if h < lo { h } else { lo };
                            hi = if h > hi { h } else { hi };
                        }
                    }
                    out.push((lo, hi));
                }
            }
            out
        };
        let vertex_bytes = if layout == Layout::Psp { core::mem::size_of::<PspVertex>() } else { core::mem::size_of::<PicaVertex>() };
        Ok(Ground { cell_y: range(per_cell), super_y: range(per_super), grid, layout, mem, vertex_bytes, u_range, per_cell, per_super, near: [Slot { key: NONE, used: 0 }; NEAR_SLOTS], small: [Slot { key: NONE, used: 0 }; SMALL_SLOTS], built: 0 })
    }

    /// A patch's vertices, from its `offset`.
    pub fn vertex(&self, offset: u32) -> *const u8 {
        unsafe { self.mem.add(offset as usize) }
    }

    /// The patch's box in the world, skirt included.
    fn bounds(&self, field: &Field, lv: u8, cx: usize, cz: usize) -> ([f32; 3], [f32; 3]) {
        let per = if lv == level::FAR { self.per_super } else { self.per_cell };
        let side = (field.n - 1) / per;
        let (lo, hi) = if lv == level::FAR { self.super_y[cz * side + cx] } else { self.cell_y[cz * side + cx] };
        let size = per as f32 * field.cell;
        let (x, z) = (field.min + cx as f32 * size, field.min + cz as f32 * size);
        ([x, lo - 0.6 * STEP[lv as usize] as f32, z], [x + size, hi + 0.01, z + size])
    }

    /// Grid point `(a, b)` of a patch: its place, its colour and its texture coordinates.
    pub fn corner(&self, field: &Field, lv: u8, cx: usize, cz: usize, a: usize, b: usize) -> (V3, [u8; 4], [f32; 2]) {
        let per = if lv == level::FAR { self.per_super } else { self.per_cell };
        let step = STEP[lv as usize];
        let (i, j) = (cx * per + a * step, cz * per + b * step);
        let c = self.grid.colors[j * field.n + i];
        let (r, g, bl) = ((c & 31) as u8, ((c >> 5) & 63) as u8, (c >> 11) as u8);
        let h = &self.grid.head;
        (
            V3 { x: field.min + i as f32 * field.cell, y: field.h[j * field.n + i], z: field.min + j as f32 * field.cell },
            [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (bl << 3) | (bl >> 2), 255],
            [a as f32 * h.u_per_square, h.v[b & 1]],
        )
    }

    /// Writes a patch's vertices at `dst`: the surface in row order, then the skirt.
    unsafe fn build(&self, field: &Field, lv: u8, cx: usize, cz: usize, min: &[f32; 3], max: &[f32; 3], dst: *mut u8) {
        let n = if lv == level::NEAR { NEAR_N } else { SMALL_N };
        let w = n + 1;
        let drop = 0.6 * STEP[lv as usize] as f32;
        let ku = 32768.0 / self.u_range;
        let ky = 65535.0 / (max[1] - min[1]);
        let step = 1.0 / pack::PICA_STEP;
        let put = |slot: usize, a: usize, b: usize, sink: f32| {
            let (p, color, uv) = self.corner(field, lv, cx, cz, a, b);
            let uv = [(uv[0] * ku) as u16, (uv[1] * 32768.0) as u16];
            match self.layout {
                Layout::Psp => {
                    let q = |t: f32| (t + 0.5) as i32 - 32768;
                    let v = PspVertex {
                        uv,
                        color: (color[0] as u16 >> 3) | ((color[1] as u16 >> 2) << 5) | ((color[2] as u16 >> 3) << 11),
                        pos: [q(a as f32 * (65535.0 / n as f32)) as i16, q((p.y - sink - min[1]) * ky) as i16, q(b as f32 * (65535.0 / n as f32)) as i16],
                    };
                    (dst as *mut PspVertex).add(slot).write_unaligned(v);
                }
                Layout::Pica => {
                    let q = |t: f32| libm::roundf(t * step) as i16;
                    let v = PicaVertex { uv: [uv[0] as i16, uv[1] as i16], color, pos: [q(p.x), q(p.y - sink), q(p.z), 0] };
                    (dst as *mut PicaVertex).add(slot).write_unaligned(v);
                }
            }
        };
        for b in 0..w {
            for a in 0..w {
                put(b * w + a, a, b, 0.0);
            }
        }
        for (k, r) in ring(n).into_iter().enumerate() {
            put(w * w + k, r as usize % w, r as usize / w, drop);
        }
    }

    /// The patch's slot: the one that holds it, or one written now.
    fn place(&mut self, field: &Field, lv: u8, cx: usize, cz: usize, frame: u32, min: &[f32; 3], max: &[f32; 3]) -> Option<(u32, bool)> {
        let key = (lv as u32) << 16 | (cz as u32) << 8 | cx as u32;
        let near = lv == level::NEAR;
        let slots: &mut [Slot] = if near { &mut self.near } else { &mut self.small };
        let stride = if near { verts(NEAR_N) } else { verts(SMALL_N) } * self.vertex_bytes;
        let base = if near { 0 } else { NEAR_SLOTS * verts(NEAR_N) * self.vertex_bytes };
        if let Some(i) = slots.iter().position(|s| s.key == key) {
            slots[i].used = frame;
            return Some(((base + i * stride) as u32, false));
        }
        // An empty slot, or the one drawn longest ago; never one a frame still in flight may read.
        let mut pick = None;
        let mut oldest = 2;
        for (i, s) in slots.iter().enumerate() {
            if s.key == NONE {
                pick = Some(i);
                break;
            }
            let age = frame.wrapping_sub(s.used);
            if age > oldest {
                oldest = age;
                pick = Some(i);
            }
        }
        let i = pick?;
        slots[i] = Slot { key, used: frame };
        unsafe { self.build(field, lv, cx, cz, min, max, self.mem.add(base + i * stride)) };
        self.built += 1;
        Some(((base + i * stride) as u32, true))
    }

    /// This frame's patches, appended to `out`.
    #[allow(clippy::too_many_arguments)]
    pub fn pick(&mut self, field: &Field, planes: &[[f32; 4]; 6], eye: V3, lod_near: f32, lod_mid: f32, lod_far: f32, frame: u32, out: &mut Vec<Patch>) {
        let supers = (field.n - 1) / self.per_super;
        let per = self.per_super / self.per_cell;
        for sz in 0..supers {
            for sx in 0..supers {
                let (min, max) = self.bounds(field, level::FAR, sx, sz);
                if !mat::visible(planes, &min, &max) {
                    continue;
                }
                let d = mat::box_distance(eye, &min, &max);
                if d > lod_mid {
                    if d <= lod_far {
                        if let Some((offset, built)) = self.place(field, level::FAR, sx, sz, frame, &min, &max) {
                            out.push(Patch { offset, level: level::FAR, built: built as u8, cx: sx as u8, cz: sz as u8, min, max, dist: d });
                        }
                    }
                    continue;
                }
                for cz in sz * per..(sz + 1) * per {
                    for cx in sx * per..(sx + 1) * per {
                        let (min, max) = self.bounds(field, level::NEAR, cx, cz);
                        if !mat::visible(planes, &min, &max) {
                            continue;
                        }
                        let dist = mat::box_distance(eye, &min, &max);
                        let lv = if dist < lod_near { level::NEAR } else { level::MID };
                        // The skirt of a middle patch drops lower.
                        let min = [min[0], min[1] - if lv == level::MID { 0.6 } else { 0.0 }, min[2]];
                        if let Some((offset, built)) = self.place(field, lv, cx, cz, frame, &min, &max) {
                            out.push(Patch { offset, level: lv, built: built as u8, cx: cx as u8, cz: cz as u8, min, max, dist });
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use requiem_sim::math::v3;

    fn field() -> Field {
        let n = 513;
        let h: Vec<f32> = (0..n * n).map(|k| ((k % n) as f32 * 0.05).sin() * 6.0 + ((k / n) as f32 * 0.031).cos() * 9.0).collect();
        Field::new(n, 4.0, -1024.0, h, Vec::new(), (0.0, 4000.0, 1.0), 940.0)
    }

    fn grid(n: usize) -> Grid {
        Grid { head: GroundHeader { n: n as u32, cell: 4.0, min: -1024.0, u_per_square: 0.25, v: [0.25, 0.0], page: 0, pad: 0 }, colors: (0..n * n).map(|k| k as u16).collect() }
    }

    fn everything() -> [[f32; 4]; 6] {
        [[0.0, 0.0, 0.0, 1.0]; 6]
    }

    #[test]
    fn the_index_list_covers_every_square_and_the_skirt() {
        for n in [NEAR_N, SMALL_N] {
            let mut idx = alloc::vec![0u16; index_count(n)];
            indices(n, &mut idx);
            assert!(idx.iter().all(|&i| (i as usize) < verts(n)));
            let mut used = alloc::vec![false; verts(n)];
            idx.iter().for_each(|&i| used[i as usize] = true);
            assert!(used.iter().all(|&u| u));
            assert_eq!(ring(n).len(), 4 * n);
        }
    }

    #[test]
    fn a_patch_lies_on_the_simulations_ground_in_both_layouts() {
        let f = field();
        for layout in [Layout::Psp, Layout::Pica] {
            let mut mem = alloc::vec![0u8; Ground::bytes(layout)];
            let mut g = unsafe { Ground::new(grid(f.n), &f, layout, 64.0, 256.0, 16.0, mem.as_mut_ptr()).unwrap() };
            let mut out = Vec::new();
            let eye = v3(10.0, 20.0, 30.0);
            g.pick(&f, &everything(), eye, 80.0, 300.0, 900.0, 1, &mut out);
            assert!(out.iter().any(|p| p.level == level::NEAR) && out.iter().any(|p| p.level == level::MID) && out.iter().any(|p| p.level == level::FAR));
            for p in &out {
                let n = if p.level == level::NEAR { NEAR_N } else { SMALL_N };
                let step = STEP[p.level as usize] as f32 * 4.0;
                let (s, t) = mat::dequant(&p.min, &p.max, 32768.0);
                for (a, b) in [(0usize, 0usize), (n, n), (3, 5), (n, 0)] {
                    let at = p.offset as usize + (b * (n + 1) + a) * g.vertex_bytes;
                    let pos = match layout {
                        Layout::Psp => {
                            let v: PspVertex = pack::read(&mem, at).unwrap();
                            v3(v.pos[0] as f32 / 32768.0 * s[0] + t[0], v.pos[1] as f32 / 32768.0 * s[1] + t[1], v.pos[2] as f32 / 32768.0 * s[2] + t[2])
                        }
                        Layout::Pica => {
                            let v: PicaVertex = pack::read(&mem, at).unwrap();
                            v3(v.pos[0] as f32 * pack::PICA_STEP, v.pos[1] as f32 * pack::PICA_STEP, v.pos[2] as f32 * pack::PICA_STEP)
                        }
                    };
                    let (x, z) = (p.min[0] + a as f32 * step, p.min[2] + b as f32 * step);
                    assert!((pos.x - x).abs() < 0.03 && (pos.z - z).abs() < 0.03, "{pos:?} vs {x} {z}");
                    assert!((pos.y - f.height(x, z)).abs() < 0.05, "{} vs {}", pos.y, f.height(x, z));
                }
            }
            // The same view again builds nothing and keeps every slot.
            let built = g.built;
            let mut again = Vec::new();
            g.pick(&f, &everything(), eye, 80.0, 300.0, 900.0, 2, &mut again);
            assert_eq!(g.built, built);
            assert_eq!(again.len(), out.len());
            assert!(again.iter().zip(&out).all(|(a, b)| a.offset == b.offset && a.built == 0));
        }
    }

    #[test]
    fn a_slot_drawn_last_frame_is_not_taken() {
        let f = field();
        let mut mem = alloc::vec![0u8; Ground::bytes(Layout::Psp)];
        let mut g = unsafe { Ground::new(grid(f.n), &f, Layout::Psp, 64.0, 256.0, 16.0, mem.as_mut_ptr()).unwrap() };
        let mut out = Vec::new();
        // Everything near: more near patches than slots.
        g.pick(&f, &everything(), v3(0.0, 0.0, 0.0), 5000.0, 6000.0, 7000.0, 1, &mut out);
        assert_eq!(out.len(), NEAR_SLOTS);
        out.clear();
        g.pick(&f, &everything(), v3(0.0, 0.0, 0.0), 5000.0, 6000.0, 7000.0, 2, &mut out);
        assert_eq!(out.len(), NEAR_SLOTS);
        assert!(out.iter().all(|p| p.built == 0));
    }
}
