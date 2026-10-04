//! The ground everything stands on: a height grid, round obstacles (trunks,
//! boulders, standing stones) in a coarse grid, the town nobody may enter and
//! the edge of the map.
//!
//! A height is read off the same two triangles per grid square that the
//! renderers draw at their finest level (the diagonal runs from a square's
//! low corner to its high corner), so a foot rests on the drawn surface.

use alloc::vec;
use alloc::vec::Vec;

use crate::math::*;

#[derive(Clone, Copy, Debug)]
pub struct Obstacle {
    pub x: f32,
    pub z: f32,
    pub r: f32,
    pub kind: u32,
}

/// Obstacle grid cell, in metres.
const BIN: f32 = 16.0;

pub struct Field {
    /// Samples per side.
    pub n: usize,
    /// Metres between samples.
    pub cell: f32,
    /// X and Z of sample 0.
    pub min: f32,
    pub h: Vec<f32>,
    pub obstacles: Vec<Obstacle>,
    bins: usize,
    start: Vec<u32>,
    items: Vec<u32>,
    /// The town behind its barrier: centre and radius.
    pub town: (f32, f32, f32),
    /// The playable square is `±half`.
    pub half: f32,
}

impl Field {
    pub fn new(n: usize, cell: f32, min: f32, h: Vec<f32>, obstacles: Vec<Obstacle>, town: (f32, f32, f32), half: f32) -> Field {
        let span = cell * (n - 1) as f32;
        let bins = (span / BIN) as usize + 1;
        let at = |o: &Obstacle| {
            let i = clamp((o.x - min) / BIN, 0.0, bins as f32 - 1.0) as usize;
            let j = clamp((o.z - min) / BIN, 0.0, bins as f32 - 1.0) as usize;
            j * bins + i
        };
        let mut start = vec![0u32; bins * bins + 1];
        for o in &obstacles {
            start[at(o) + 1] += 1;
        }
        for i in 0..bins * bins {
            start[i + 1] += start[i];
        }
        let mut fill = start.clone();
        let mut items = vec![0u32; obstacles.len()];
        for (k, o) in obstacles.iter().enumerate() {
            let c = at(o);
            items[fill[c] as usize] = k as u32;
            fill[c] += 1;
        }
        Field { n, cell, min, h, obstacles, bins, start, items, town, half }
    }

    /// A flat field with nothing on it, for tests.
    pub fn flat(half: f32) -> Field {
        let n = 9;
        Field::new(n, half * 2.0 / (n - 1) as f32, -half, vec![0.0; n * n], Vec::new(), (0.0, half * 4.0, 1.0), half - 4.0)
    }

    #[inline]
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let last = (self.n - 1) as f32 - 0.001;
        let fx = clamp((x - self.min) / self.cell, 0.0, last);
        let fz = clamp((z - self.min) / self.cell, 0.0, last);
        let (i, j) = (fx as usize, fz as usize);
        let (u, v) = (fx - i as f32, fz - j as f32);
        let at = j * self.n + i;
        let (h00, h10, h01, h11) = (self.h[at], self.h[at + 1], self.h[at + self.n], self.h[at + self.n + 1]);
        if u >= v {
            h00 + (h10 - h00) * u + (h11 - h10) * v
        } else {
            h00 + (h11 - h01) * u + (h01 - h00) * v
        }
    }

    /// The surface normal, from the heights a cell to each side.
    pub fn normal(&self, x: f32, z: f32) -> V3 {
        let e = self.cell;
        v3(self.height(x - e, z) - self.height(x + e, z), 2.0 * e, self.height(x, z - e) - self.height(x, z + e)).norm()
    }

    pub fn point(&self, x: f32, z: f32) -> V3 {
        v3(x, self.height(x, z), z)
    }

    /// Moves a disc of radius `r` out of the obstacles, the town and the map's edge.
    pub fn push_out(&self, x: &mut f32, z: &mut f32, r: f32) {
        let i = clamp((*x - self.min) / BIN, 0.0, self.bins as f32 - 1.0) as isize;
        let j = clamp((*z - self.min) / BIN, 0.0, self.bins as f32 - 1.0) as isize;
        for dj in -1..=1 {
            for di in -1..=1 {
                let (ci, cj) = (i + di, j + dj);
                if ci < 0 || cj < 0 || ci >= self.bins as isize || cj >= self.bins as isize {
                    continue;
                }
                let c = cj as usize * self.bins + ci as usize;
                for &k in &self.items[self.start[c] as usize..self.start[c + 1] as usize] {
                    let o = &self.obstacles[k as usize];
                    let (dx, dz) = (*x - o.x, *z - o.z);
                    let reach = o.r + r;
                    let d2 = dx * dx + dz * dz;
                    if d2 < reach * reach {
                        let d = sqrt(d2);
                        if d > 1e-4 {
                            *x = o.x + dx / d * reach;
                            *z = o.z + dz / d * reach;
                        } else {
                            *x = o.x + reach;
                        }
                    }
                }
            }
        }
        let (dx, dz) = (*x - self.town.0, *z - self.town.1);
        let reach = self.town.2 + r;
        let d2 = dx * dx + dz * dz;
        if d2 < reach * reach {
            let d = sqrt(d2).max(1e-3);
            *x = self.town.0 + dx / d * reach;
            *z = self.town.1 + dz / d * reach;
        }
        *x = clamp(*x, -self.half, self.half);
        *z = clamp(*z, -self.half, self.half);
    }
}
