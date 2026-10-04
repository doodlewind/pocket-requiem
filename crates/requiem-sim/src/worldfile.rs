//! The simulation's world file (`RQSW`): the height grid, the obstacles, where
//! the mage starts, where the demon stands and where each cohort of the army
//! forms up. The generator writes it (`web/src/world/worldfile.ts`); every
//! host hands the same bytes to `load`.
//!
//! Layout, little-endian:
//!
//! ```text
//! u32 magic "RQSW", u32 version
//! u32 n, f32 cell, f32 min, f32 h_min, f32 h_scale
//! u16 heights[n × n], padded to 4 bytes
//! u32 count; { f32 x, z, r; u32 kind } obstacles
//! f32 town x, z, r; f32 half
//! f32 start x, z, yaw; f32 demon x, z, yaw; f32 gate x, z
//! u32 count; { f32 x, z, yaw, spacing; u16 cols, rows; u8 mix, captain; u16 pad } cohorts
//! ```

use alloc::vec::Vec;

use crate::field::{Field, Obstacle};
use crate::sim::Sim;

pub const MAGIC: u32 = 0x5753_5152;
pub const VERSION: u32 = 1;

/// Where a cohort forms up: its front rank's centre, the way it faces, its ranks and files.
#[derive(Clone, Copy, Debug)]
pub struct Muster {
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
    pub spacing: f32,
    pub cols: u16,
    pub rows: u16,
    /// 0 swords, 1 halberds, 2 greatswords, 3 mixed.
    pub mix: u8,
    pub captain: u8,
}

pub struct Stage {
    pub start: (f32, f32, f32),
    pub demon: (f32, f32, f32),
    pub gate: (f32, f32),
    pub musters: Vec<Muster>,
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], &'static str> {
        if self.at + n > self.b.len() {
            return Err("world file is truncated");
        }
        let s = &self.b[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, &'static str> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u16(&mut self) -> Result<u16, &'static str> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u8(&mut self) -> Result<u8, &'static str> {
        Ok(self.take(1)?[0])
    }
    fn f32(&mut self) -> Result<f32, &'static str> {
        Ok(f32::from_bits(self.u32()?))
    }
}

pub fn parse(bytes: &[u8]) -> Result<(Field, Stage), &'static str> {
    let mut r = Reader { b: bytes, at: 0 };
    if r.u32()? != MAGIC {
        return Err("not a world file");
    }
    if r.u32()? != VERSION {
        return Err("world file version");
    }
    let n = r.u32()? as usize;
    let (cell, min, h_min, h_scale) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?);
    if !(2..=4097).contains(&n) {
        return Err("height grid size");
    }
    let raw = r.take(n * n * 2)?;
    let h: Vec<f32> = raw.chunks_exact(2).map(|c| h_min + u16::from_le_bytes([c[0], c[1]]) as f32 * h_scale).collect();
    r.at = (r.at + 3) & !3;
    let count = r.u32()? as usize;
    let mut obstacles = Vec::with_capacity(count);
    for _ in 0..count {
        obstacles.push(Obstacle { x: r.f32()?, z: r.f32()?, r: r.f32()?, kind: r.u32()? });
    }
    let town = (r.f32()?, r.f32()?, r.f32()?);
    let half = r.f32()?;
    let start = (r.f32()?, r.f32()?, r.f32()?);
    let demon = (r.f32()?, r.f32()?, r.f32()?);
    let gate = (r.f32()?, r.f32()?);
    let count = r.u32()? as usize;
    let mut musters = Vec::with_capacity(count);
    for _ in 0..count {
        let (x, z, yaw, spacing) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?);
        let (cols, rows) = (r.u16()?, r.u16()?);
        let (mix, captain) = (r.u8()?, r.u8()?);
        r.u16()?;
        musters.push(Muster { x, z, yaw, spacing, cols, rows, mix, captain });
    }
    Ok((Field::new(n, cell, min, h, obstacles, town, half), Stage { start, demon, gate, musters }))
}

pub fn load(bytes: &[u8]) -> Result<Sim, &'static str> {
    let (field, stage) = parse(bytes)?;
    Ok(Sim::new(field, stage))
}

/// A flat test stage: the mage in the middle, `cohorts` blocks of 8 × 6 ahead of her.
pub fn test_stage(cohorts: usize) -> (Field, Stage) {
    let field = Field::flat(400.0);
    let mut musters = Vec::new();
    for i in 0..cohorts {
        let row = (i / 4) as f32;
        let col = (i % 4) as f32 - 1.5;
        musters.push(Muster { x: col * 26.0, z: -40.0 - row * 22.0, yaw: core::f32::consts::PI, spacing: 1.9, cols: 8, rows: 6, mix: (i % 4) as u8, captain: 1 });
    }
    (field, Stage { start: (0.0, 0.0, 0.0), demon: (0.0, -300.0, core::f32::consts::PI), gate: (0.0, 200.0), musters })
}
