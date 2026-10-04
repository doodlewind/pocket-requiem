//! The device pack (`RQPK`): what the world compiler writes and a runtime
//! loads in one read.
//!
//! Little-endian. Header: `"RQPK"`, version, section count, zero; then one
//! 16-byte entry per section (tag, offset, size, zero). Sections start on
//! 16-byte boundaries.
//!
//! | Tag    | Contents |
//! | ------ | -------- |
//! | `META` | JSON: scene constants, identity, statistics |
//! | `TEX0` | the atlas: `TexHeader`, then BC1 levels, largest first, block rows in order |
//! | `MESH` | `MeshRec` table |
//! | `VTX0` | `Vertex` records of every static mesh |
//! | `IDX0` | `u16` indices, relative to each mesh's first vertex |
//! | `MODL` | skinned models: count, then per model `ModelHeader`, `SkinVertex` records, `u16` indices padded to 4 bytes |
//! | `FONT` | interface glyphs: `FontHeader`, `Glyph` table, 8-bit coverage |
//! | `SIMW` | the simulation's world file, unchanged |
//!
//! A handheld pack (PSP, 3DS) has the same container and these sections:
//!
//! | Tag    | Contents |
//! | ------ | -------- |
//! | `META` | JSON, for tools; the runtime does not read it |
//! | `HSCN` | `HandScene`: scene constants and the pack's layout switches |
//! | `TEX0` | `TexHeader`, then per atlas page (its palette, for an indexed format, and) its levels, largest first, each padded to 16 bytes |
//! | `HMSH` | `HandMesh` table |
//! | `VTX0` | resident vertices (`PspVertex` or `PicaVertex`) |
//! | `IDX0` | resident `u16` indices |
//! | `GRND` | `GroundHeader`, then the ground's baked colour per grid point; the device builds the ground from it and the heights in `SIMW` |
//! | `CLIP` | PSP: per mesh, its large triangles' groups (`ClipGroup`) and one byte per large triangle, the distance (× 2 m) inside which the CPU clips it |
//! | `MODL` | skinned models, cut into draws of at most 19 bones of `SkinVertex` (3DS) or 4 bones of `PspSkinVertex` (PSP) |
//! | `CRWP` | PSP: the army's frames, each stored with the next of its clip, for the GE's vertex blend |
//! | `CRWD` | 3DS: the army's frames, as in the Vita pack |
//! | `FXPK` | the effects' templates and constants, without the atlas |
//! | `FXTX` | the effects' atlas: `TexHeader` and one level (PSP: 8-bit indices of a grey ramp; 3DS: 8-bit alpha) |
//! | `FONT` | `FontHeader`, `Glyph` table, then the glyph atlas as a 16-bit device texture |
//! | `SIMW` | the simulation's world file, unchanged |
//! | `MAPT` | 3DS: the field from above, 240 × 240 texels of `r << 11 | g << 5 | b` |

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const MAGIC: u32 = u32::from_le_bytes(*b"RQPK");
pub const VERSION: u32 = 2;

pub const fn tag(t: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*t)
}
pub const META: u32 = tag(b"META");
pub const TEX0: u32 = tag(b"TEX0");
pub const MESH: u32 = tag(b"MESH");
pub const VTX0: u32 = tag(b"VTX0");
pub const IDX0: u32 = tag(b"IDX0");
pub const MODL: u32 = tag(b"MODL");
pub const FONT: u32 = tag(b"FONT");
pub const SIMW: u32 = tag(b"SIMW");
/// The army's baked frames.
pub const CRWD: u32 = tag(b"CRWD");
/// The army's frames for the PSP: each stored frame interleaved with the next of its clip.
pub const CRWP: u32 = tag(b"CRWP");
/// The effects' atlas in the device's texture format.
pub const FXTX: u32 = tag(b"FXTX");
/// Compiled effects.
pub const FXPK: u32 = tag(b"FXPK");
pub const HSCN: u32 = tag(b"HSCN");
pub const HMSH: u32 = tag(b"HMSH");
pub const NEAR: u32 = tag(b"NEAR");
pub const CLIP: u32 = tag(b"CLIP");
pub const SIMG: u32 = tag(b"SIMG");
pub const MAPT: u32 = tag(b"MAPT");
/// The ground's baked colours as a grid: `GroundHeader`, then one `u16` per grid point.
pub const GRND: u32 = tag(b"GRND");

/// Which mesh of a place in the grid a record is.
pub mod mesh_kind {
    /// A 64 m cell with its detailed geometry.
    pub const NEAR: u32 = 0;
    /// The same cell with its simple geometry.
    pub const MID: u32 = 1;
    /// A 256 m super-cell's far geometry.
    pub const FAR: u32 = 2;
    /// Always drawn.
    pub const BACKDROP: u32 = 3;
}

/// One static mesh: a range of vertices and indices and its bounds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct MeshRec {
    pub kind: u32,
    pub cx: i32,
    pub cz: i32,
    pub vtx_first: u32,
    pub vtx_count: u32,
    pub idx_first: u32,
    pub idx_count: u32,
    /// Positions dequantize as `min + q / 65535 × (max - min)`.
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub pad: u32,
}

/// Static vertex, 16 bytes: position `u16 × 3` normalized over the mesh's
/// bounds, texture coordinates `i16 × 2` normalized (`u / UV_SCALE`, `v`),
/// colour `u8 × 4` (baked light × tint, half scale, sRGB-encoded).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Vertex {
    pub pos: [u16; 3],
    pub pad: u16,
    pub uv: [i16; 2],
    pub color: [u8; 4],
}

/// `u` is stored divided by this, so eight texture repeats fit an `i16`.
pub const UV_SCALE: f32 = 8.0;
/// Colours are stored at half scale: a shader multiplies by this.
pub const COLOR_SCALE: f32 = 2.0;

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct TexHeader {
    pub width: u32,
    pub height: u32,
    pub mips: u32,
    /// 1: BC1.
    pub format: u32,
}

/// `TexHeader::format` values.
pub mod tex_format {
    /// BC1 blocks in row order (PS Vita).
    pub const BC1: u32 = 1;
    /// BC1 blocks with the PSP's layout: the index word, then the two colours.
    pub const PSP_DXT1: u32 = 2;
    /// 16-bit `r | g << 5 | b << 11`, swizzled.
    pub const PSP_5650: u32 = 3;
    /// 16-bit `r | g << 4 | b << 8 | a << 12`, swizzled.
    pub const PSP_4444: u32 = 4;
    /// 16-bit `r << 11 | g << 5 | b`, in 8 × 8 tiles of Morton order, rows bottom-up.
    pub const PICA_RGB565: u32 = 5;
    /// 16-bit `r << 12 | g << 8 | b << 4 | a`, tiled the same way.
    pub const PICA_RGBA4: u32 = 6;
    /// 8-bit indices, swizzled. Each page starts with its palette: 256 colours, `r, g, b, 255` bytes.
    pub const PSP_T8: u32 = 7;
    /// 8-bit alpha, in 8 × 8 tiles of Morton order, rows bottom-up.
    pub const PICA_A8: u32 = 8;
    /// Bytes of a `PSP_T8` page's palette.
    pub const PALETTE_BYTES: usize = 1024;

    /// Bytes of one level of `w × h` texels.
    pub fn level_bytes(format: u32, w: u32, h: u32) -> usize {
        match format {
            BC1 | PSP_DXT1 => (w.div_ceil(4) * h.div_ceil(4) * 8) as usize,
            PSP_T8 | PICA_A8 => (w * h) as usize,
            _ => (w * h * 2) as usize,
        }
    }
}

/// The ground of a handheld pack. Its heights are the simulation's grid (`SIMW`): `n × n` points, `cell`
/// metres apart, from `min` on both axes. A device builds the ground's meshes from the two grids: a
/// vertex's `u` is its column × `u_per_square`, its `v` is `v[0]` on even rows and `v[1]` on odd ones
/// (on atlas page `page`), and its colour is this section's entry for its grid point: baked light × tint
/// at half scale, `r | g << 5 | b << 11`.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct GroundHeader {
    pub n: u32,
    pub cell: f32,
    pub min: f32,
    pub u_per_square: f32,
    pub v: [f32; 2],
    pub page: u32,
    pub pad: u32,
}

/// Scene constants and layout switches of a handheld pack. Colours are linear.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct HandScene {
    pub sun_dir: [f32; 3],
    /// Haze is `1 - exp(-(depth × fog_density)²)` where the device computes it per vertex.
    pub fog_density: f32,
    pub sun: [f32; 3],
    /// Cells nearer than this draw their detailed mesh.
    pub lod_near: f32,
    pub sky: [f32; 3],
    /// Super-cells nearer than this draw their cells; farther ones draw their far mesh.
    pub lod_mid: f32,
    pub bounce: [f32; 3],
    /// Super-cells farther than this are not drawn.
    pub lod_far: f32,
    pub fog: [f32; 3],
    pub clip_near: f32,
    pub horizon: [f32; 3],
    pub clip_far: f32,
    pub zenith: [f32; 3],
    pub cell: f32,
    pub glow: [f32; 3],
    pub super_cell: f32,
    /// Linear haze for fixed-function devices: none at `fog_near`, full at `fog_far`.
    pub fog_near: f32,
    pub fog_far: f32,
    /// The moon's disc: its angular radius in radians.
    pub moon_radius: f32,
    /// The most knights out of formation at once on this machine (a whole number).
    pub crowd_free: f32,
    /// Stored `u` covers `0..u_range` texture repeats.
    pub u_range: f32,
    pub color_scale: f32,
    pub screen: [f32; 2],
    /// Atlas pages, stacked along `v`: page `p` holds `v` in `p / pages .. (p + 1) / pages`.
    pub pages: u32,
    /// 1: detailed cell meshes live in `NEAR` and are read on demand.
    pub near_streamed: u32,
    /// Levels of detail of a knight in the pack.
    pub crowd_lods: u32,
    /// Triangles of knights a frame may draw.
    pub crowd_budget: u32,
    /// The moon's colour.
    pub moon: [f32; 3],
    /// The most knights one frame draws.
    pub crowd_max: u32,
    /// The distance in metres at which each level of detail hands over to the next; the last used entry is where a knight is no longer drawn.
    pub crowd_reach: [f32; 8],
}

/// One static mesh of a handheld pack.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct HandMesh {
    pub kind: u32,
    pub cx: i32,
    pub cz: i32,
    /// The atlas page its triangles sample.
    pub page: u32,
    /// First vertex in `VTX0`; for a streamed mesh, the byte offset of its vertices in `NEAR`.
    pub vtx_first: u32,
    pub vtx_count: u32,
    /// First index in `IDX0`; for a streamed mesh, the byte offset of its indices in `NEAR`.
    pub idx_first: u32,
    pub idx_count: u32,
    /// Indices from here to the end are large triangles, three each.
    pub big_first: u32,
    /// Where their groups and distance bytes start in `CLIP`.
    pub clip_first: u32,
    /// The largest of their clip distances, in metres; 0 without large triangles.
    pub clip_radius: f32,
    /// The mesh's bounds. PSP positions dequantize as `min + (q + 32768) / 65535 × (max - min)`.
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub pad: u32,
}

/// PSP: a mesh's large triangles are stored in groups of neighbours. A mesh's bytes in `CLIP` are a
/// `u32` group count, the groups, then one distance byte per large triangle (group after group, each
/// group's largest first), padded to 4 bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct ClipGroup {
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// Large triangles in the group.
    pub tris: u32,
}

/// PSP static vertex, 12 bytes, in the order the GE reads components:
/// texture coordinates `u16 × 2` (`u / u_range`, `v` within the page, × 32768),
/// colour 5650, position `i16 × 3`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct PspVertex {
    pub uv: [u16; 2],
    pub color: u16,
    pub pos: [i16; 3],
}

/// Metres per unit of a 3DS static vertex's position. Every cell's mesh is on
/// one grid for the whole world, so a frame's draws share one transform and a
/// vertex two meshes share lands on the same point in both.
pub const PICA_STEP: f32 = 1.0 / 24.0;
/// The same for the backdrop mesh, which reaches farther than that grid.
pub const PICA_BACKDROP_STEP: f32 = 0.25;

/// 3DS static vertex, 16 bytes: the same texture coordinates as `i16`, colour
/// `u8 × 4`, position `i16 × 3` in `PICA_STEP` units from the world's origin,
/// and a pad the loader reads as `w`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct PicaVertex {
    pub uv: [i16; 2],
    pub color: [u8; 4],
    pub pos: [i16; 4],
}

/// Bones one PSP draw can blend.
pub const PSP_BATCH_BONES: usize = 4;

/// One draw of a PSP skinned model: the bones it blends, then `vtx_count`
/// `PspSkinVertex` and `idx_count` `u16` indices padded to 4 bytes.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct PspBatch {
    pub bone_count: u32,
    pub vtx_count: u32,
    pub idx_count: u32,
    pub bones: [u8; PSP_BATCH_BONES],
}

/// PSP skinned vertex, 24 bytes, in GE order: four weights (128 is one) for the
/// batch's bones, colour 8888, normal `i8 × 3`, bind-pose position.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct PspSkinVertex {
    pub weights: [u8; PSP_BATCH_BONES],
    pub color: [u8; 4],
    pub normal: [i8; 4],
    pub pos: [f32; 3],
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct ModelHeader {
    /// 0: the player; 10 to 12: the giants' builds; 20 to 22: the same, coarse.
    pub id: u32,
    pub vtx_count: u32,
    pub idx_count: u32,
    /// A PSP model: the number of `PspBatch` draws that follow instead of one vertex and index block.
    pub pad: u32,
}

/// Skinned vertex, 24 bytes: bind-pose position, normal `i8 × 3` normalized,
/// sRGB tint, two bone indices and their weights (`u8` normalized, summing to 255).
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct SkinVertex {
    pub pos: [f32; 3],
    pub normal: [i8; 4],
    pub color: [u8; 4],
    pub bones: [u8; 2],
    pub weights: [u8; 2],
}

// ---------------------------------------------------------------------- the army
//
// A knight is not skinned at play time. Each kind's clips are sampled into
// stored frames; for each level of detail every vertex is placed at every
// frame and written as a `CrowdVertex`. A device draws a knight as a blend of
// two frames, and draws every knight that shows the same two frames in one
// instanced call.
//
// Section layout: `CrowdHeader`, then `kinds × lods` `CrowdMesh` records, then
// the data they point into (offsets from the section's start, each a multiple of 16).

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct CrowdHeader {
    pub kinds: u32,
    pub lods: u32,
    /// Stored frames per kind.
    pub frames: u32,
    /// Metres that a position's full 16-bit range stands for.
    pub scale: f32,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct CrowdMesh {
    pub kind: u32,
    pub lod: u32,
    pub vtx_count: u32,
    pub idx_count: u32,
    /// `vtx_count` colours: sRGB tint, and in alpha how much the surface is bare metal.
    pub color_at: u32,
    /// `idx_count` 16-bit indices.
    pub idx_at: u32,
    /// `frames × vtx_count` `CrowdVertex`, frame after frame.
    pub frames_at: u32,
    pub pad: u32,
}

/// One vertex of one stored frame: 12 bytes.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct CrowdVertex {
    /// Position in the figure's frame, over `±CrowdHeader::scale`.
    pub pos: [i16; 3],
    pub pad: u16,
    pub normal: [i8; 4],
}

/// PSP: one vertex of one stored frame, as the GE reads a morph target: colour 5650 with the light baked in, then position.
/// A mesh's frame `f` is `vtx_count` pairs: this frame's vertex, then the same vertex in the next frame of its clip.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct PspCrowdVertex {
    pub color: u16,
    pub pos: [i16; 3],
}

/// One knight in an instanced call: 20 bytes.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct CrowdInstance {
    pub pos: [f32; 3],
    /// Sine and cosine of the heading.
    pub turn: [i16; 2],
    /// Blend toward the second frame, a strike's flash, size above 1 (in units of 1/255 × 1), unused.
    pub blend: u8,
    pub flash: u8,
    pub grow: u8,
    pub pad: u8,
}


// ---------------------------------------------------------------------- effects
//
// Section layout: `FxHeader`, then `effects` `FxEffect` records, `layers`
// `FxLayer` records, the atlas (`atlas × atlas` bytes of brightness), and the
// templates and indices the layers point into (offsets from the section's start).

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct FxHeader {
    pub effects: u32,
    pub layers: u32,
    /// Side of the square atlas in texels, and where its bytes start.
    pub atlas: u32,
    pub atlas_at: u32,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct FxEffect {
    pub first: u32,
    pub count: u32,
}

/// One layer of an effect: which program places it, how it blends, its template and its constants.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct FxLayer {
    /// 0 particles, 1 ring, 2 ribbon, 3 shell.
    pub program: u32,
    /// 0 add, 1 over.
    pub blend: u32,
    pub vtx_count: u32,
    pub idx_count: u32,
    /// `vtx_count` `FxVertex`.
    pub vtx_at: u32,
    pub idx_at: u32,
    pub pad: [u32; 2],
    /// Eight rows of four constants (`uP`).
    pub rows: [f32; 32],
}

/// A template vertex: three signed-normalized vectors.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct FxVertex {
    pub a: [i8; 4],
    pub b: [i8; 4],
    pub c: [i8; 4],
}

/// One live effect in an instanced call: place and age (0 to 1), direction and its number.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct FxInstance {
    pub pos: [f32; 3],
    pub age: f32,
    pub dir: [f32; 3],
    pub a: f32,
}


#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct FontHeader {
    pub width: u32,
    pub height: u32,
    pub glyphs: u32,
    /// 0: 8-bit coverage follows the glyph table; otherwise a `tex_format` texture does.
    pub pad: u32,
}

/// One glyph at one size. Sizes are pixel heights at 960 × 544.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct Glyph {
    pub code: u16,
    pub size: u16,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub left: i16,
    /// From the baseline up to the glyph's top row.
    pub top: i16,
    pub advance: f32,
}

/// Bytes of a `repr(C)` value without padding surprises: every struct here is
/// made of 4-byte-aligned fields with explicit padding.
pub fn bytes_of<T: Copy>(v: &T) -> &[u8] {
    unsafe { core::slice::from_raw_parts(v as *const T as *const u8, core::mem::size_of::<T>()) }
}
pub fn slice_bytes<T: Copy>(v: &[T]) -> &[u8] {
    unsafe { core::slice::from_raw_parts(v.as_ptr() as *const u8, core::mem::size_of_val(v)) }
}
/// Reads a `repr(C)` record at `at`; the bytes need not be aligned.
pub fn read<T: Copy>(b: &[u8], at: usize) -> Option<T> {
    let n = core::mem::size_of::<T>();
    let s = b.get(at..at + n)?;
    Some(unsafe { core::ptr::read_unaligned(s.as_ptr() as *const T) })
}

/// A parsed pack over borrowed bytes.
pub struct Pack<'a> {
    pub bytes: &'a [u8],
    sections: Vec<(u32, usize, usize)>,
}

impl<'a> Pack<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Pack<'a>, String> {
        let word = |i: usize| read::<u32>(bytes, i).ok_or_else(|| "pack is truncated".to_string());
        if word(0)? != MAGIC {
            return Err("not a pack".into());
        }
        if word(4)? != VERSION {
            return Err(format!("pack version {} (this build reads {VERSION})", word(4)?));
        }
        let n = word(8)? as usize;
        let mut sections = Vec::with_capacity(n);
        for i in 0..n {
            let at = 16 + i * 16;
            let (t, off, size) = (word(at)?, word(at + 4)? as usize, word(at + 8)? as usize);
            if off + size > bytes.len() {
                return Err("pack section runs past the end".into());
            }
            sections.push((t, off, size));
        }
        Ok(Pack { bytes, sections })
    }
    pub fn section(&self, t: u32) -> Result<&'a [u8], String> {
        self.sections.iter().find(|s| s.0 == t).map(|s| &self.bytes[s.1..s.1 + s.2]).ok_or_else(|| format!("pack has no {} section", String::from_utf8_lossy(&t.to_le_bytes())))
    }
    /// Offset and size of a section in the pack, for reading it from storage.
    pub fn range(&self, t: u32) -> Option<(usize, usize)> {
        self.sections.iter().find(|s| s.0 == t).map(|s| (s.1, s.2))
    }
    pub fn hand_meshes(&self) -> Result<Vec<HandMesh>, String> {
        let b = self.section(HMSH)?;
        let n = core::mem::size_of::<HandMesh>();
        Ok((0..b.len() / n).filter_map(|i| read::<HandMesh>(b, i * n)).collect())
    }
    pub fn meshes(&self) -> Result<Vec<MeshRec>, String> {
        let b = self.section(MESH)?;
        let n = core::mem::size_of::<MeshRec>();
        Ok((0..b.len() / n).filter_map(|i| read::<MeshRec>(b, i * n)).collect())
    }
}

/// Assembles a pack from sections.
pub fn write(sections: &[(u32, &[u8])]) -> Vec<u8> {
    let head = 16 + sections.len() * 16;
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(sections.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    let mut at = (head + 15) & !15;
    for (t, data) in sections {
        out.extend_from_slice(&t.to_le_bytes());
        out.extend_from_slice(&(at as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        at = (at + data.len() + 15) & !15;
    }
    for (_, data) in sections {
        while out.len() % 16 != 0 {
            out.push(0);
        }
        out.extend_from_slice(data);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts() {
        assert_eq!(core::mem::size_of::<Vertex>(), 16);
        assert_eq!(core::mem::size_of::<MeshRec>(), 56);
        assert_eq!(core::mem::size_of::<SkinVertex>(), 24);
        assert_eq!(core::mem::size_of::<Glyph>(), 20);
        assert_eq!(core::mem::size_of::<HandScene>(), 224);
        assert_eq!(core::mem::size_of::<PspCrowdVertex>(), 8);
        assert_eq!(core::mem::size_of::<CrowdVertex>(), 12);
        assert_eq!(core::mem::size_of::<FxLayer>(), 160);
        assert_eq!(core::mem::size_of::<HandMesh>(), 72);
        assert_eq!(core::mem::size_of::<PspVertex>(), 12);
        assert_eq!(core::mem::size_of::<ClipGroup>(), 28);
        assert_eq!(core::mem::size_of::<PicaVertex>(), 16);
        assert_eq!(core::mem::size_of::<PspBatch>(), 16);
        assert_eq!(core::mem::size_of::<PspSkinVertex>(), 24);
    }

    #[test]
    fn round_trip() {
        let rec = MeshRec { kind: mesh_kind::FAR, cx: -2, cz: 3, vtx_count: 9, ..Default::default() };
        let pack = write(&[(META, b"{}"), (MESH, bytes_of(&rec))]);
        let p = Pack::parse(&pack).unwrap();
        assert_eq!(p.section(META).unwrap(), b"{}");
        assert_eq!(p.meshes().unwrap(), vec![rec]);
        assert!(p.section(TEX0).is_err());
    }
}
