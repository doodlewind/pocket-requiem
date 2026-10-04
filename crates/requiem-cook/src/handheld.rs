//! Lowering for the handhelds (PSP, 3DS).
//!
//! The same baked geometry as the Vita pack, cut to what each machine reads
//! without conversion:
//!
//! - The ground is not stored as meshes. Its heights are the simulation's grid
//!   and its baked colours a second grid (`GRND`); the device builds the meshes
//!   of the cells it draws. That is a megabyte instead of the 860 000
//!   triangles the Vita pack holds.
//! - The atlas becomes pages of at most 512 × 512 texels, stacked along `v`;
//!   every mesh is split by the page its triangles sample.
//! - Vertices are quantized into the device's own layout (`PspVertex`,
//!   `PicaVertex`).
//! - PSP: large triangles sort to the end of each mesh with a distance in
//!   `CLIP`; inside that distance the runtime clips them on the CPU, because
//!   the GE drops a triangle with a vertex outside its 4096-pixel guard band
//!   instead of clipping it.
//! - Skinned models are cut into draws of the bones one draw can hold: four
//!   on the PSP (the GE's blend), nineteen on the 3DS (a program's uniforms).
//! - PSP: the army's frames are stored in pairs, so the GE blends a frame
//!   with the next (`CRWP`). 3DS: the frames are the Vita's (`CRWD`).
//! - The effects' atlas is stored in the device's texture format (`FXTX`).

use crate::ir::{self, Mesh, STRIDE};
use requiem_pack::{self as pack, mesh_kind, tex_format, CrowdHeader, CrowdMesh, FontHeader, FxHeader, GroundHeader, HandMesh, HandScene, ModelHeader, PicaVertex, PspBatch, PspCrowdVertex, PspSkinVertex, PspVertex, SkinVertex, TexHeader, PSP_BATCH_BONES};
use rayon::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Handheld {
    /// Texels of one atlas page.
    pub page: [u32; 2],
    pub lod: Lod,
    pub fog: Fog,
    pub clip: Clip,
    pub crowd: Crowd,
    pub models: Models,
    /// Side of the effects' atlas on the device, in texels.
    pub fx_atlas: u32,
    /// Which of the source's props a cell draws up close: `near` or `mid`.
    pub props: String,
    /// A triangle with an edge longer than this many metres is a large triangle (PSP).
    #[serde(default)]
    pub big_edge: f32,
    /// Detailed cell meshes are read on demand (PSP).
    #[serde(default)]
    pub stream_near: bool,
}
#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Lod {
    pub near: f32,
    pub mid: f32,
    pub far: f32,
}
#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Fog {
    pub near: f32,
    pub far: f32,
    pub density: f32,
}
#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub near: f32,
    pub far: f32,
}
/// The army: which exported levels of detail the pack carries, nearest first, the distance at which each hands
/// over to the next (the last is where meshes end and far figures begin), the triangles of knights a frame
/// may draw, and the most knights it draws as meshes.
#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Crowd {
    pub lods: Vec<u32>,
    pub reach: Vec<f32>,
    pub budget: u32,
    pub max: u32,
    /// The most knights out of formation at once: what the machine's processor simulates in a frame.
    pub free: u32,
}
/// Source model ids.
#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Models {
    pub mage: i32,
    pub demon: i32,
}

/// Texture repeats a stored `u` covers.
pub const U_RANGE: f32 = 16.0;
/// A triangle farther than this many of its longest edges cannot reach from inside
/// the view to outside the guard band (see `requiem_handheld::clip`).
pub const CLIP_REACH: f32 = 3.3;

#[derive(Clone, Copy, PartialEq)]
pub enum Target {
    Psp,
    Pica,
}

pub struct Lowered {
    pub rec: HandMesh,
    /// Device vertex bytes.
    pub vtx: Vec<u8>,
    pub idx: Vec<u16>,
    /// The mesh's bytes in `CLIP` (empty without large triangles).
    pub clip: Vec<u8>,
    /// Large triangles.
    pub big: usize,
}

/// Splits the parts of one cell and level by atlas page and quantizes each into a device mesh.
pub fn lower(parts: &[(&Mesh, &Vec<[u8; 4]>)], kind: u32, cx: i32, cz: i32, h: &Handheld, pages: u32, target: Target, limit: usize) -> Result<Vec<Lowered>, String> {
    struct Build<'a> {
        src: Vec<(&'a [f32], [u8; 4])>,
        tris: Vec<[u32; 3]>,
    }
    let mut builds: Vec<Build> = (0..pages).map(|_| Build { src: Vec::new(), tris: Vec::new() }).collect();
    for (mesh, colors) in parts {
        let mut maps: Vec<Vec<u32>> = (0..pages).map(|_| vec![u32::MAX; mesh.verts.len() / STRIDE]).collect();
        for tri in mesh.idx.chunks_exact(3) {
            let v = |i: u32| mesh.verts[i as usize * STRIDE + 7];
            let centre = (v(tri[0]) + v(tri[1]) + v(tri[2])) / 3.0;
            let page = ((centre * pages as f32) as u32).min(pages - 1);
            let (lo, hi) = (page as f32 / pages as f32, (page + 1) as f32 / pages as f32);
            if tri.iter().any(|&i| v(i) < lo - 1e-4 || v(i) > hi + 1e-4) {
                return Err(format!("a triangle in cell {cx},{cz} samples two atlas pages"));
            }
            let b = &mut builds[page as usize];
            let mut out = [0u32; 3];
            for (k, &i) in tri.iter().enumerate() {
                let slot = &mut maps[page as usize][i as usize];
                if *slot == u32::MAX {
                    *slot = b.src.len() as u32;
                    b.src.push((&mesh.verts[i as usize * STRIDE..(i as usize + 1) * STRIDE], colors[i as usize]));
                }
                out[k] = *slot;
            }
            b.tris.push(out);
        }
    }
    let mut out = Vec::new();
    for (page, b) in builds.into_iter().enumerate() {
        if b.tris.is_empty() {
            continue;
        }
        if b.src.len() > limit {
            return Err(format!("cell {cx},{cz} page {page} has {} vertices; the profile allows {limit}", b.src.len()));
        }
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        let mut u_min = f32::MAX;
        for (v, _) in &b.src {
            for a in 0..3 {
                min[a] = min[a].min(v[a]);
                max[a] = max[a].max(v[a]);
            }
            u_min = u_min.min(v[6]);
        }
        for a in 0..3 {
            if max[a] - min[a] < 0.01 {
                max[a] = min[a] + 0.01;
            }
        }
        // `u` repeats, so a whole number of repeats comes off for free and the rest is unsigned.
        let u_shift = u_min.floor();
        let mut vtx = Vec::with_capacity(b.src.len() * 16);
        let step = if kind == mesh_kind::BACKDROP { pack::PICA_BACKDROP_STEP } else { pack::PICA_STEP };
        for (v, color) in &b.src {
            let mut pos = [0i16; 4];
            for a in 0..3 {
                pos[a] = match target {
                    // Over the mesh's own bounds.
                    Target::Psp => (((v[a] - min[a]) / (max[a] - min[a]) * 65535.0 + 0.5).clamp(0.0, 65535.0) as i32 - 32768) as i16,
                    // On the world's grid.
                    Target::Pica => {
                        let q = (v[a] / step).round();
                        if q.abs() > 32767.0 {
                            return Err(format!("a vertex of cell {cx},{cz} at {} m is outside the 3DS position grid", v[a]));
                        }
                        q as i16
                    }
                };
            }
            let u = (v[6] - u_shift) / U_RANGE;
            if !(0.0..1.0).contains(&u) {
                return Err(format!("texture coordinate u spans more than {U_RANGE} repeats in cell {cx},{cz}"));
            }
            let vv = (v[7] * pages as f32 - page as f32).clamp(0.0, 1.0);
            let uv = [(u * 32768.0).round().min(32767.0) as u16, (vv * 32768.0).round().min(32767.0) as u16];
            match target {
                Target::Psp => {
                    let c = (color[0] as u16 >> 3) | ((color[1] as u16 >> 2) << 5) | ((color[2] as u16 >> 3) << 11);
                    vtx.extend_from_slice(pack::bytes_of(&PspVertex { uv, color: c, pos: [pos[0], pos[1], pos[2]] }));
                }
                Target::Pica => vtx.extend_from_slice(pack::bytes_of(&PicaVertex { uv: [uv[0] as i16, uv[1] as i16], color: *color, pos })),
            }
        }
        // Small triangles first; large ones after, in groups of neighbours (at most 4 × 4 over the mesh,
        // no finer than 16 m), the largest first inside a group. A runtime measures its distance to each
        // group and tests the prefix of the group that could reach the guard band from there.
        let p3 = |i: u32| {
            let v = b.src[i as usize].0;
            [v[0], v[1], v[2]]
        };
        let edge = |t: &[u32; 3]| {
            let d = |a: [f32; 3], c: [f32; 3]| ((a[0] - c[0]).powi(2) + (a[1] - c[1]).powi(2) + (a[2] - c[2]).powi(2)).sqrt();
            let (a, c, e) = (p3(t[0]), p3(t[1]), p3(t[2]));
            d(a, c).max(d(c, e)).max(d(e, a))
        };
        let size = ((max[0] - min[0]).max(max[2] - min[2]) / 4.0).max(16.0);
        let mut small = Vec::new();
        let mut big: Vec<((u32, u32), f32, [u32; 3])> = Vec::new();
        for t in &b.tris {
            let e = edge(t);
            if h.big_edge > 0.0 && e > h.big_edge {
                let (a, c, d) = (p3(t[0]), p3(t[1]), p3(t[2]));
                let centre = [(a[0] + c[0] + d[0]) / 3.0, (a[2] + c[2] + d[2]) / 3.0];
                let key = (((centre[1] - min[2]) / size) as u32, ((centre[0] - min[0]) / size) as u32);
                big.push((key, e, *t));
            } else {
                small.push(*t);
            }
        }
        let code_of = |e: f32| ((e * CLIP_REACH / 2.0).ceil() as u32).clamp(1, 255) as u8;
        big.sort_by_key(|t| (t.0, 255 - code_of(t.1)));
        let mut idx: Vec<u16> = small.iter().flatten().map(|&i| i as u16).collect();
        let big_first = idx.len() as u32;
        let mut groups: Vec<pack::ClipGroup> = Vec::new();
        let mut codes = Vec::with_capacity(big.len());
        let mut clip_radius = 0.0f32;
        let mut last = None;
        for (key, e, t) in &big {
            if last != Some(*key) {
                groups.push(pack::ClipGroup { min: [f32::MAX; 3], max: [f32::MIN; 3], tris: 0 });
                last = Some(*key);
            }
            let g = groups.last_mut().unwrap();
            for &i in t {
                let p = p3(i);
                for a in 0..3 {
                    g.min[a] = g.min[a].min(p[a]);
                    g.max[a] = g.max[a].max(p[a]);
                }
            }
            g.tris += 1;
            idx.extend(t.iter().map(|&i| i as u16));
            let code = code_of(*e);
            codes.push(code);
            clip_radius = clip_radius.max(if code == 255 { 1e9 } else { code as f32 * 2.0 });
        }
        let mut clip = Vec::new();
        if !big.is_empty() {
            clip.extend_from_slice(&(groups.len() as u32).to_le_bytes());
            clip.extend_from_slice(pack::slice_bytes(&groups));
            clip.extend_from_slice(&codes);
            while clip.len() % 4 != 0 {
                clip.push(0);
            }
        }
        let big = big.len();
        out.push(Lowered {
            rec: HandMesh { kind, cx, cz, page: page as u32, vtx_first: 0, vtx_count: b.src.len() as u32, idx_first: 0, idx_count: idx.len() as u32, big_first, clip_first: 0, clip_radius, min, max, pad: 0 },
            vtx,
            idx,
            clip,
            big,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------- textures

fn psp_swizzle(src: &[u8], row_bytes: usize, rows: usize) -> Vec<u8> {
    let stride = (row_bytes + 15) & !15;
    let padded = (rows + 7) & !7;
    let mut out = vec![0u8; stride * padded];
    for y in 0..rows {
        for x in (0..row_bytes).step_by(16) {
            let n = (row_bytes - x).min(16);
            let dst = ((y / 8) * (stride / 16) + x / 16) * 128 + (y % 8) * 16;
            out[dst..dst + n].copy_from_slice(&src[y * row_bytes + x..y * row_bytes + x + n]);
        }
    }
    out
}

/// 16-bit texels into the PICA's layout: 8 × 8 tiles in row order, Morton order
/// inside a tile, and the image's last row first.
fn pica_tile(texels: &[u16], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 2];
    for y in 0..h {
        for x in 0..w {
            let fy = h - 1 - y;
            let tile = (fy / 8) * (w / 8) + x / 8;
            let (lx, ly) = (x % 8, fy % 8);
            let mut m = 0;
            for b in 0..3 {
                m |= ((lx >> b) & 1) << (2 * b);
                m |= ((ly >> b) & 1) << (2 * b + 1);
            }
            let o = (tile * 64 + m) * 2;
            out[o..o + 2].copy_from_slice(&texels[y * w + x].to_le_bytes());
        }
    }
    out
}

/// 8-bit texels in the PICA's layout.
fn pica_tile8(texels: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let fy = h - 1 - y;
            let tile = (fy / 8) * (w / 8) + x / 8;
            let (lx, ly) = (x % 8, fy % 8);
            let mut m = 0;
            for b in 0..3 {
                m |= ((lx >> b) & 1) << (2 * b);
                m |= ((ly >> b) & 1) << (2 * b + 1);
            }
            out[tile * 64 + m] = texels[y * w + x];
        }
    }
    out
}

fn encode(format: u32, rgba: &[u8], w: usize, h: usize) -> Result<Vec<u8>, String> {
    let px = |i: usize| (rgba[i * 4] as u16, rgba[i * 4 + 1] as u16, rgba[i * 4 + 2] as u16, rgba[i * 4 + 3] as u16);
    match format {
        tex_format::PSP_DXT1 => {
            let fmt = texpresso::Format::Bc1;
            let mut out = vec![0u8; fmt.compressed_size(w, h)];
            // Every texel opaque: the encoder must not pick the three-colour mode's transparent index.
            let opaque: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
            fmt.compress(&opaque, w, h, texpresso::Params { algorithm: texpresso::Algorithm::ClusterFit, ..Default::default() }, &mut out);
            // The GE reads a block as the index word, then the two colours.
            for b in out.chunks_exact_mut(8) {
                let (colors, lines) = ([b[0], b[1], b[2], b[3]], [b[4], b[5], b[6], b[7]]);
                b[..4].copy_from_slice(&lines);
                b[4..].copy_from_slice(&colors);
            }
            Ok(out)
        }
        tex_format::PSP_5650 | tex_format::PSP_4444 => {
            let mut bytes = Vec::with_capacity(w * h * 2);
            for i in 0..w * h {
                let (r, g, b, a) = px(i);
                let t = if format == tex_format::PSP_5650 { (r >> 3) | ((g >> 2) << 5) | ((b >> 3) << 11) } else { (r >> 4) | ((g >> 4) << 4) | ((b >> 4) << 8) | ((a >> 4) << 12) };
                bytes.extend_from_slice(&t.to_le_bytes());
            }
            if w < 8 || h < 8 {
                return Err(format!("a swizzled level of {w} × {h} texels is smaller than a block"));
            }
            Ok(psp_swizzle(&bytes, w * 2, h))
        }
        tex_format::PICA_RGB565 | tex_format::PICA_RGBA4 => {
            if w < 8 || h < 8 {
                return Err(format!("a tiled level of {w} × {h} texels is smaller than a tile"));
            }
            let texels: Vec<u16> = (0..w * h)
                .map(|i| {
                    let (r, g, b, a) = px(i);
                    if format == tex_format::PICA_RGB565 {
                        ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)
                    } else {
                        ((r >> 4) << 12) | ((g >> 4) << 8) | ((b >> 4) << 4) | (a >> 4)
                    }
                })
                .collect();
            Ok(pica_tile(&texels, w, h))
        }
        f => Err(format!("texture format {f} is not a handheld format")),
    }
}

pub fn format_of(name: &str) -> Result<u32, String> {
    Ok(match name {
        "psp-dxt1" => tex_format::PSP_DXT1,
        "psp-5650" => tex_format::PSP_5650,
        "psp-t8" => tex_format::PSP_T8,
        "pica-rgb565" => tex_format::PICA_RGB565,
        other => return Err(format!("texture format {other:?} is not one of psp-dxt1, psp-5650, psp-t8, pica-rgb565")),
    })
}

/// A 256-colour palette for a page by median cut over its largest level, and a table from
/// 5-bit-per-channel colour to the nearest palette entry.
fn palette(rgba: &[u8]) -> (Vec<[u8; 3]>, Vec<u8>) {
    let bin = |p: &[u8]| ((p[0] as usize >> 3) << 10) | ((p[1] as usize >> 3) << 5) | (p[2] as usize >> 3);
    let mut count = vec![0u32; 32768];
    for p in rgba.chunks_exact(4) {
        count[bin(p)] += 1;
    }
    let used: Vec<u16> = (0..32768u32).filter(|&i| count[i as usize] > 0).map(|i| i as u16).collect();
    let channel = |i: u16, c: usize| ((i >> (10 - 5 * c)) & 31) as u32;
    // Boxes of used bins; split the box with the most pixels along its longest channel at the pixel median.
    let mut boxes: Vec<Vec<u16>> = vec![used];
    while boxes.len() < 256 {
        let Some(at) = (0..boxes.len()).filter(|&b| boxes[b].len() > 1).max_by_key(|&b| boxes[b].iter().map(|&i| count[i as usize] as u64).sum::<u64>()) else { break };
        let mut b = boxes.swap_remove(at);
        let range = |c: usize| {
            let (lo, hi) = b.iter().fold((31, 0), |(lo, hi), &i| (lo.min(channel(i, c)), hi.max(channel(i, c))));
            hi - lo
        };
        let c = (0..3).max_by_key(|&c| range(c)).unwrap();
        b.sort_by_key(|&i| channel(i, c));
        let total: u64 = b.iter().map(|&i| count[i as usize] as u64).sum();
        let mut acc = 0u64;
        let mut cut = 1;
        for (k, &i) in b.iter().enumerate() {
            acc += count[i as usize] as u64;
            if acc * 2 >= total {
                cut = (k + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let tail = b.split_off(cut);
        boxes.push(b);
        boxes.push(tail);
    }
    let colors: Vec<[u8; 3]> = boxes
        .iter()
        .map(|b| {
            let n: u64 = b.iter().map(|&i| count[i as usize] as u64).sum::<u64>().max(1);
            [0, 1, 2].map(|c| (b.iter().map(|&i| (channel(i, c) * 8 + 4) as u64 * count[i as usize] as u64).sum::<u64>() / n) as u8)
        })
        .collect();
    let nearest: Vec<u8> = (0..32768u32)
        .into_par_iter()
        .map(|i| {
            let p = [0, 1, 2].map(|c| (channel(i as u16, c) * 8 + 4) as i32);
            (0..colors.len()).min_by_key(|&k| (0..3).map(|c| (colors[k][c] as i32 - p[c]).pow(2)).sum::<i32>()).unwrap_or(0) as u8
        })
        .collect();
    (colors, nearest)
}

/// `TexHeader` of one page, then every page's levels.
pub fn atlas(rgba: &[u8], w: usize, h: usize, edges: &[usize], page: [u32; 2], format: u32, max_mips: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let (pw, ph) = (page[0] as usize, page[1] as usize);
    if !pw.is_power_of_two() || !ph.is_power_of_two() || pw > w || w % pw != 0 {
        return Err(format!("atlas page {pw} × {ph} does not divide the {w} × {h} atlas"));
    }
    let base = (w / pw).trailing_zeros();
    let scaled_h = h >> base;
    if scaled_h % ph != 0 {
        return Err(format!("atlas page height {ph} does not divide the atlas at that scale ({scaled_h})"));
    }
    let pages = scaled_h / ph;
    for p in 1..pages {
        if !edges.contains(&(p * ph << base)) {
            return Err(format!("the boundary of atlas page {p} cuts a texture strip"));
        }
    }
    // A swizzled block is 16 bytes by 8 rows: 16 texels across at one byte each.
    let min_side = match format {
        tex_format::PSP_DXT1 => 4,
        tex_format::PSP_T8 => 16,
        _ => 8,
    };
    let mut levels = 0;
    while levels < max_mips && (pw >> levels) >= min_side && (ph >> levels) >= min_side {
        levels += 1;
    }
    let mips: Vec<(Vec<u8>, usize, usize)> = (0..levels).into_par_iter().map(|l| crate::texture::mip(rgba, w, h, edges, base + l)).collect();
    let mut out = pack::bytes_of(&TexHeader { width: pw as u32, height: ph as u32, mips: levels, format }).to_vec();
    for p in 0..pages {
        let indexed = if format == tex_format::PSP_T8 {
            let (colors, nearest) = palette(&mips[0].0[p * ph * pw * 4..(p + 1) * ph * pw * 4]);
            while out.len() % 16 != 0 {
                out.push(0);
            }
            for k in 0..256 {
                let c = colors.get(k).copied().unwrap_or([0, 0, 0]);
                out.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
            Some(nearest)
        } else {
            None
        };
        for (l, (px, mw, _)) in mips.iter().enumerate() {
            let (lw, lh) = (pw >> l, ph >> l);
            debug_assert_eq!(*mw, lw);
            let rows = &px[p * lh * lw * 4..(p + 1) * lh * lw * 4];
            let bytes = match &indexed {
                Some(nearest) => {
                    let idx: Vec<u8> = rows.chunks_exact(4).map(|t| nearest[((t[0] as usize >> 3) << 10) | ((t[1] as usize >> 3) << 5) | (t[2] as usize >> 3)]).collect();
                    psp_swizzle(&idx, lw, lh)
                }
                None => encode(format, rows, lw, lh)?,
            };
            debug_assert_eq!(bytes.len(), tex_format::level_bytes(format, lw as u32, lh as u32));
            while out.len() % 16 != 0 {
                out.push(0);
            }
            out.extend_from_slice(&bytes);
        }
    }
    Ok((out, pages as u32, levels))
}

/// The glyph atlas as a 16-bit texture: white, coverage in alpha, and a solid 2 × 2 block in the last corner.
pub fn font(a8: &[u8], target: Target) -> Result<Vec<u8>, String> {
    let head: FontHeader = pack::read(a8, 0).ok_or("font header")?;
    let table = core::mem::size_of::<FontHeader>() + head.glyphs as usize * core::mem::size_of::<pack::Glyph>();
    let (w, h) = (head.width as usize, head.height as usize);
    let mut rgba = vec![255u8; w * h * 4];
    for i in 0..w * h {
        rgba[i * 4 + 3] = a8[table + i];
    }
    for (x, y) in [(w - 1, h - 1), (w - 2, h - 1), (w - 1, h - 2), (w - 2, h - 2)] {
        rgba[(y * w + x) * 4 + 3] = 255;
    }
    let format = if target == Target::Psp { tex_format::PSP_4444 } else { tex_format::PICA_RGBA4 };
    let mut out = pack::bytes_of(&FontHeader { pad: format, ..head }).to_vec();
    out.extend_from_slice(&a8[core::mem::size_of::<FontHeader>()..table]);
    while out.len() % 16 != 0 {
        out.push(0);
    }
    out.extend_from_slice(&encode(format, &rgba, w, h)?);
    Ok(out)
}

// ---------------------------------------------------------------- models

struct SrcVertex {
    pos: [f32; 3],
    normal: [i8; 4],
    color: [u8; 4],
    bones: [u8; 2],
    w0: u8,
}

fn src_vertices(m: &Mesh) -> Vec<SrcVertex> {
    const S: usize = ir::SKIN_STRIDE;
    (0..m.verts.len() / S)
        .map(|i| {
            let v = &m.verts[i * S..(i + 1) * S];
            let c = |x: f32| (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            let n = |x: f32| (x.clamp(-1.0, 1.0) * 127.0).round() as i8;
            SrcVertex { pos: [v[0], v[1], v[2]], normal: [n(v[3]), n(v[4]), n(v[5]), 0], color: [c(v[6]), c(v[7]), c(v[8]), 255], bones: [v[9] as u8, v[10] as u8], w0: (v[11].clamp(0.0, 1.0) * 255.0 + 0.5) as u8 }
        })
        .collect()
}

/// One model as bone batches for the GE.
fn psp_model(id: u32, m: &Mesh) -> Result<Vec<u8>, String> {
    let verts = src_vertices(m);
    struct Batch {
        bones: Vec<u8>,
        verts: Vec<PspSkinVertex>,
        idx: Vec<u16>,
        /// (source vertex, rigid on its first bone) to batch vertex.
        map: HashMap<(u32, bool), u16>,
    }
    // A vertex's bones that carry weight.
    let used = |v: &SrcVertex, rigid: bool| -> Vec<u8> {
        if rigid || v.w0 >= 254 || v.bones[0] == v.bones[1] {
            vec![v.bones[0]]
        } else if v.w0 <= 1 {
            vec![v.bones[1]]
        } else {
            vec![v.bones[0], v.bones[1]]
        }
    };
    let mut tris: Vec<[u32; 3]> = m.idx.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
    tris.sort_by_key(|t| t.iter().map(|&i| verts[i as usize].bones[0]).min());
    let mut batches: Vec<Batch> = Vec::new();
    for t in tris {
        // More bones than one draw blends: the vertex with the weakest second bone follows its first bone alone.
        let mut rigid = [false; 3];
        let set = |rigid: &[bool; 3]| {
            let mut s: Vec<u8> = t.iter().zip(rigid).flat_map(|(&i, &r)| used(&verts[i as usize], r)).collect();
            s.sort();
            s.dedup();
            s
        };
        let mut bones = set(&rigid);
        while bones.len() > PSP_BATCH_BONES {
            let k = (0..3).filter(|&k| !rigid[k]).min_by_key(|&k| 255 - verts[t[k] as usize].w0).ok_or("a triangle's bones cannot be reduced")?;
            rigid[k] = true;
            bones = set(&rigid);
        }
        let growth = |b: &Batch| bones.iter().filter(|x| !b.bones.contains(x)).count();
        let at = match batches.iter().enumerate().filter(|(_, b)| b.bones.len() + growth(b) <= PSP_BATCH_BONES && b.verts.len() + 3 <= 65535).min_by_key(|(_, b)| growth(b)) {
            Some((i, _)) => i,
            None => {
                batches.push(Batch { bones: Vec::new(), verts: Vec::new(), idx: Vec::new(), map: HashMap::new() });
                batches.len() - 1
            }
        };
        let b = &mut batches[at];
        for x in &bones {
            if !b.bones.contains(x) {
                b.bones.push(*x);
            }
        }
        for (k, &i) in t.iter().enumerate() {
            let v = &verts[i as usize];
            let r = rigid[k] || used(v, false).len() == 1;
            let key = (i, r);
            let slot = match b.map.get(&key) {
                Some(&s) => s,
                None => {
                    let mut weights = [0u8; PSP_BATCH_BONES];
                    let at = |bone: u8| b.bones.iter().position(|&x| x == bone).unwrap();
                    if r {
                        weights[at(used(v, rigid[k])[0])] = 128;
                    } else {
                        let w = ((v.w0 as u32 * 128 + 127) / 255) as u8;
                        weights[at(v.bones[0])] = w;
                        weights[at(v.bones[1])] += 128 - w;
                    }
                    let s = b.verts.len() as u16;
                    b.verts.push(PspSkinVertex { weights, color: v.color, normal: v.normal, pos: v.pos });
                    b.map.insert(key, s);
                    s
                }
            };
            b.idx.push(slot);
        }
    }
    let head = ModelHeader { id, vtx_count: batches.iter().map(|b| b.verts.len() as u32).sum(), idx_count: m.idx.len() as u32, pad: batches.len() as u32 };
    let mut out = pack::bytes_of(&head).to_vec();
    for b in &batches {
        let mut bones = [0u8; PSP_BATCH_BONES];
        bones[..b.bones.len()].copy_from_slice(&b.bones);
        out.extend_from_slice(pack::bytes_of(&PspBatch { bone_count: b.bones.len() as u32, vtx_count: b.verts.len() as u32, idx_count: b.idx.len() as u32, bones }));
        out.extend_from_slice(pack::slice_bytes(&b.verts));
        out.extend_from_slice(pack::slice_bytes(&b.idx));
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    Ok(out)
}

/// Bones one PICA draw can hold: three uniform rows each.
pub const PICA_BATCH_BONES: usize = 19;

/// One model as draws of at most `PICA_BATCH_BONES` bones. The header's `pad` is the number of draws; each is
/// 32 bytes (bone count, vertex count, index count, a spare, then up to 20 bone numbers), its `SkinVertex`
/// vertices and its indices. A vertex's bone fields hold its bones' first uniform row in the draw (slot × 3),
/// ready for the shader's address register.
fn pica_model(id: u32, m: &Mesh) -> Result<Vec<u8>, String> {
    let verts = src_vertices(m);
    struct Batch {
        bones: Vec<u8>,
        verts: Vec<SkinVertex>,
        idx: Vec<u16>,
        map: HashMap<u32, u16>,
    }
    let mut tris: Vec<[u32; 3]> = m.idx.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
    tris.sort_by_key(|t| t.iter().map(|&i| verts[i as usize].bones[0]).min());
    let mut batches: Vec<Batch> = Vec::new();
    for t in tris {
        let mut bones: Vec<u8> = t.iter().flat_map(|&i| verts[i as usize].bones).collect();
        bones.sort();
        bones.dedup();
        let growth = |b: &Batch| bones.iter().filter(|x| !b.bones.contains(x)).count();
        let at = match batches.iter().enumerate().filter(|(_, b)| b.bones.len() + growth(b) <= PICA_BATCH_BONES && b.verts.len() + 3 <= 65535).min_by_key(|(_, b)| growth(b)) {
            Some((i, _)) => i,
            None => {
                batches.push(Batch { bones: Vec::new(), verts: Vec::new(), idx: Vec::new(), map: HashMap::new() });
                batches.len() - 1
            }
        };
        let b = &mut batches[at];
        for x in &bones {
            if !b.bones.contains(x) {
                b.bones.push(*x);
            }
        }
        for &i in &t {
            let slot = match b.map.get(&i) {
                Some(&s) => s,
                None => {
                    let v = &verts[i as usize];
                    let row = |bone: u8| (b.bones.iter().position(|&x| x == bone).unwrap() * 3) as u8;
                    let s = b.verts.len() as u16;
                    b.verts.push(SkinVertex { pos: v.pos, normal: v.normal, color: v.color, bones: [row(v.bones[0]), row(v.bones[1])], weights: [v.w0, 255 - v.w0] });
                    b.map.insert(i, s);
                    s
                }
            };
            b.idx.push(slot);
        }
    }
    let head = ModelHeader { id, vtx_count: batches.iter().map(|b| b.verts.len() as u32).sum(), idx_count: m.idx.len() as u32, pad: batches.len() as u32 };
    let mut out = pack::bytes_of(&head).to_vec();
    for b in &batches {
        out.extend_from_slice(&(b.bones.len() as u32).to_le_bytes());
        out.extend_from_slice(&(b.verts.len() as u32).to_le_bytes());
        out.extend_from_slice(&(b.idx.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        let mut bones = [0u8; 20];
        bones[..b.bones.len()].copy_from_slice(&b.bones);
        out.extend_from_slice(&bones);
        out.extend_from_slice(pack::slice_bytes(&b.verts));
        out.extend_from_slice(pack::slice_bytes(&b.idx));
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    Ok(out)
}

/// The profile's models under the ids a runtime looks for: 0 the mage, 4 the demon.
pub fn models(ir: &ir::Ir, which: &Models, target: Target) -> Result<(Vec<u8>, serde_json::Value), String> {
    let roles = [(0u32, which.mage), (4u32, which.demon)];
    let mut out = (roles.len() as u32).to_le_bytes().to_vec();
    let mut stats = Vec::new();
    for (id, source) in roles {
        let m = ir.models.iter().find(|m| m.head[0] == source).ok_or(format!("the source has no model {source}"))?;
        let bytes = if target == Target::Psp { psp_model(id, m)? } else { pica_model(id, m)? };
        let head: ModelHeader = pack::read(&bytes, 0).unwrap();
        stats.push(serde_json::json!({"id": id, "source": source, "triangles": m.idx.len() / 3, "vertices": head.vtx_count, "draws": head.pad.max(1)}));
        out.extend_from_slice(&bytes);
    }
    Ok((out, serde_json::Value::Array(stats)))
}

pub fn scene(scene: &serde_json::Value, h: &Handheld, screen: [u32; 2], pages: u32) -> Result<HandScene, String> {
    let arr = |k: &str| -> Result<[f32; 3], String> {
        let v = scene[k].as_array().filter(|a| a.len() == 3).ok_or(format!("scene.json: {k}"))?;
        Ok([v[0].as_f64().unwrap_or(0.0) as f32, v[1].as_f64().unwrap_or(0.0) as f32, v[2].as_f64().unwrap_or(0.0) as f32])
    };
    let num = |v: &serde_json::Value, k: &str| v.as_f64().map(|x| x as f32).ok_or(format!("scene.json: {k}"));
    if h.crowd.reach.len() != h.crowd.lods.len() || h.crowd.lods.is_empty() || h.crowd.lods.len() > 8 {
        return Err("the profile's crowd needs one reach per level of detail, and at most eight levels".into());
    }
    let mut crowd_reach = [0.0f32; 8];
    crowd_reach[..h.crowd.reach.len()].copy_from_slice(&h.crowd.reach);
    Ok(HandScene {
        sun_dir: arr("sunDir")?,
        fog_density: h.fog.density,
        sun: arr("sun")?,
        lod_near: h.lod.near,
        sky: arr("sky")?,
        lod_mid: h.lod.mid,
        bounce: arr("bounce")?,
        lod_far: h.lod.far,
        fog: arr("fog")?,
        clip_near: h.clip.near,
        horizon: arr("horizon")?,
        clip_far: h.clip.far,
        zenith: arr("zenith")?,
        cell: num(&scene["cell"], "cell")?,
        glow: arr("glow")?,
        super_cell: num(&scene["superCell"], "superCell")?,
        fog_near: h.fog.near,
        fog_far: h.fog.far,
        moon_radius: num(&scene["moonRadius"], "moonRadius")?,
        crowd_free: h.crowd.free as f32,
        u_range: U_RANGE,
        color_scale: pack::COLOR_SCALE,
        screen: [screen[0] as f32, screen[1] as f32],
        pages,
        near_streamed: h.stream_near as u32,
        crowd_lods: h.crowd.lods.len() as u32,
        crowd_budget: h.crowd.budget,
        moon: arr("moon")?,
        crowd_max: h.crowd.max,
        crowd_reach,
    })
}

/// PSP: the army's frames as the GE blends them. A draw with two morph targets reads each vertex as the pair
/// (this frame, the next frame of the clip), so every stored frame is written interleaved with its successor.
/// The GE lights nothing here: the moon and the sky are baked into each frame's colours, for a knight that
/// faces south as the army does.
pub fn crowd_psp(ir: &ir::Ir, lods: &[u32], scene: &ir::Scene) -> Result<(Vec<u8>, serde_json::Value), String> {
    use requiem_sim::anim::skin;
    use requiem_sim::knight::{self, Knight, FRAMES};
    use requiem_sim::math::*;
    const S: usize = ir::SKIN_STRIDE;
    const SCALE: f32 = 4.0;
    let pad16 = |out: &mut Vec<u8>| {
        while out.len() % 16 != 0 {
            out.push(0);
        }
    };
    let moon = v3(scene.sun_dir[0], scene.sun_dir[1], scene.sun_dir[2]).norm();
    let head = core::mem::size_of::<CrowdHeader>() + 3 * lods.len() * core::mem::size_of::<CrowdMesh>();
    let mut data = vec![0u8; head];
    pad16(&mut data);
    let mut recs = Vec::new();
    let mut stats = Vec::new();
    for kind in 1..=3u32 {
        let knight = Knight::new(kind);
        let bind_inv = knight.skel.bind_inverse();
        for (level, &source) in lods.iter().enumerate() {
            let id = (kind * 100 + source) as i32;
            let model = ir.models.iter().find(|m| m.head[0] == id).ok_or(format!("the source has no knight model {id}"))?;
            let nv = model.verts.len() / S;
            if nv > 65535 {
                return Err(format!("knight model {id} has {nv} vertices; indices are 16-bit"));
            }
            // Every frame's vertices: lit colour and position.
            let frames: Vec<Vec<PspCrowdVertex>> = (0..FRAMES as u16)
                .into_par_iter()
                .map(|f| {
                    let mats = skin(&knight.frame(f), &bind_inv);
                    (0..nv)
                        .map(|i| {
                            let v = &model.verts[i * S..(i + 1) * S];
                            let (p, n) = (v3(v[0], v[1], v[2]), v3(v[3], v[4], v[5]));
                            let (a, b, wa) = (v[9] as usize, v[10] as usize, v[11]);
                            let pos = mats[a].apply(p) * wa + mats[b].apply(p) * (1.0 - wa);
                            let nrm = (mats[a].r.apply(n) * wa + mats[b].r.apply(n) * (1.0 - wa)).norm_or(V3::UP);
                            // The army faces south: the figure's -Z is the world's +Z.
                            let world = v3(-nrm.x, nrm.y, -nrm.z);
                            let ndl = world.dot(moon).max(0.0);
                            let up = 0.5 + 0.5 * world.y;
                            // As the Vita's program lights a knight: the tint times the encoded light, and on bare
                            // metal a gleam. The program's gleam depends on the eye (a highlight off the moon, a rim
                            // against the sky); baked, it is the rim at a glancing third.
                            let bare = crate::crowd::metal(&v[6..9]) as f32 / 255.0;
                            let mut c = [0u16; 3];
                            for k in 0..3 {
                                let light = scene.sun[k] * ndl + scene.bounce[k] + (scene.sky[k] - scene.bounce[k]) * up;
                                let gleam = (scene.sun[k] * 0.07 * bare + scene.sky[k] * 0.15).powf(1.0 / 2.2) * 0.8 * (0.35 + 0.65 * bare);
                                c[k] = ((v[6 + k].max(0.0) * light.max(0.0).powf(1.0 / 2.2) + gleam).min(1.0) * 255.0 + 0.5) as u16;
                            }
                            let q = |x: f32| (x.clamp(-SCALE, SCALE) / SCALE * 32767.0).round() as i16;
                            PspCrowdVertex { color: (c[0] >> 3) | ((c[1] >> 2) << 5) | ((c[2] >> 3) << 11), pos: [q(pos.x), q(pos.y), q(pos.z)] }
                        })
                        .collect()
                })
                .collect();
            let idx_at = data.len() as u32;
            for &i in &model.idx {
                data.extend_from_slice(&(i as u16).to_le_bytes());
            }
            pad16(&mut data);
            let frames_at = data.len() as u32;
            for f in 0..FRAMES as u16 {
                let (this, next) = (&frames[f as usize], &frames[knight::next(f) as usize]);
                for i in 0..nv {
                    data.extend_from_slice(pack::bytes_of(&this[i]));
                    data.extend_from_slice(pack::bytes_of(&next[i]));
                }
            }
            pad16(&mut data);
            recs.push(CrowdMesh { kind: kind - 1, lod: level as u32, vtx_count: nv as u32, idx_count: model.idx.len() as u32, color_at: 0, idx_at, frames_at, pad: 0 });
            stats.push(serde_json::json!({"kind": kind, "lod": level, "source": source, "vertices": nv, "triangles": model.idx.len() / 3, "frameBytes": nv * FRAMES * 16}));
        }
    }
    let header = CrowdHeader { kinds: 3, lods: lods.len() as u32, frames: FRAMES as u32, scale: SCALE };
    let mut at = 0;
    data[at..at + core::mem::size_of::<CrowdHeader>()].copy_from_slice(pack::bytes_of(&header));
    at += core::mem::size_of::<CrowdHeader>();
    for r in &recs {
        data[at..at + core::mem::size_of::<CrowdMesh>()].copy_from_slice(pack::bytes_of(r));
        at += core::mem::size_of::<CrowdMesh>();
    }
    Ok((data, serde_json::json!({"frames": FRAMES, "meshes": stats})))
}

/// The effects' atlas at `side` texels, in the device's format: `TexHeader`, then one level. The PSP's is 8-bit
/// indices, swizzled, for a palette in which entry `i` is brightness `i`; the 3DS's is 8-bit alpha, tiled.
pub fn fx_atlas(fxpk: &[u8], side: u32, target: Target) -> Result<Vec<u8>, String> {
    let head: FxHeader = pack::read(fxpk, 0).ok_or("effects header")?;
    let (n, s) = (head.atlas as usize, side as usize);
    if s == 0 || n % s != 0 || !s.is_power_of_two() {
        return Err(format!("an effects atlas of {side} texels does not divide the source's {n}"));
    }
    let src = &fxpk[head.atlas_at as usize..head.atlas_at as usize + n * n];
    let k = n / s;
    let mut small = vec![0u8; s * s];
    for y in 0..s {
        for x in 0..s {
            let mut sum = 0u32;
            for j in 0..k {
                for i in 0..k {
                    sum += src[(y * k + j) * n + x * k + i] as u32;
                }
            }
            small[y * s + x] = (sum / (k * k) as u32) as u8;
        }
    }
    let (format, data) = if target == Target::Psp { (tex_format::PSP_T8, psp_swizzle(&small, s, s)) } else { (tex_format::PICA_A8, pica_tile8(&small, s, s)) };
    let mut out = pack::bytes_of(&TexHeader { format, width: side, height: side, mips: 1 }).to_vec();
    out.extend_from_slice(&data);
    Ok(out)
}

/// A super-cell's far props as one mesh per cell: each triangle goes to the cell its centre is in.
pub fn cut_by_cell(m: &Mesh, colors: &[[u8; 4]], cell: f32) -> Vec<(Mesh, Vec<[u8; 4]>)> {
    struct Part {
        verts: Vec<f32>,
        colors: Vec<[u8; 4]>,
        idx: Vec<u32>,
        map: HashMap<u32, u32>,
    }
    let mut parts: std::collections::BTreeMap<(i32, i32), Part> = Default::default();
    for t in m.idx.chunks_exact(3) {
        let at = |i: u32, a: usize| m.verts[i as usize * STRIDE + a];
        let (x, z) = ((at(t[0], 0) + at(t[1], 0) + at(t[2], 0)) / 3.0, (at(t[0], 2) + at(t[1], 2) + at(t[2], 2)) / 3.0);
        let p = parts.entry(((z / cell).floor() as i32, (x / cell).floor() as i32)).or_insert_with(|| Part { verts: Vec::new(), colors: Vec::new(), idx: Vec::new(), map: HashMap::new() });
        for &i in t {
            let slot = match p.map.get(&i) {
                Some(&s) => s,
                None => {
                    let s = p.colors.len() as u32;
                    p.verts.extend_from_slice(&m.verts[i as usize * STRIDE..(i as usize + 1) * STRIDE]);
                    p.colors.push(colors[i as usize]);
                    p.map.insert(i, s);
                    s
                }
            };
            p.idx.push(slot);
        }
    }
    parts.into_iter().map(|((cz, cx), p)| (Mesh { head: [ir::layer::FAR, cx, cz], verts: p.verts, idx: p.idx }, p.colors)).collect()
}

/// `GRND`: the baked colour of every point of the ground's grid, read off the source's 4 m ground meshes.
pub fn ground(ir: &ir::Ir, colors: &[Vec<[u8; 4]>], field: &requiem_sim::field::Field, pages: u32) -> Result<Vec<u8>, String> {
    let n = field.n;
    let mut grid = vec![0u16; n * n];
    let mut seen = vec![false; n * n];
    for (b, col) in ir.buckets.iter().zip(colors).filter(|(b, _)| b.head[0] == ir::layer::GROUND_NEAR) {
        for (i, c) in col.iter().enumerate() {
            let v = &b.verts[i * STRIDE..(i + 1) * STRIDE];
            let (gx, gz) = ((v[0] - field.min) / field.cell, (v[2] - field.min) / field.cell);
            let (ix, iz) = (gx.round() as usize, gz.round() as usize);
            if (gx - ix as f32).abs() > 0.01 || (gz - iz as f32).abs() > 0.01 || ix >= n || iz >= n {
                return Err("a ground vertex is off the simulation's grid".into());
            }
            // A skirt's vertex is below the surface; the surface's own vertex has the colour.
            let on = (v[1] - field.h[iz * n + ix]).abs() < 0.05;
            if on || !seen[iz * n + ix] {
                grid[iz * n + ix] = (c[0] as u16 >> 3) | ((c[1] as u16 >> 2) << 5) | ((c[2] as u16 >> 3) << 11);
                seen[iz * n + ix] |= on;
            }
        }
    }
    if seen.iter().any(|s| !s) {
        return Err("the source's ground does not cover the simulation's grid".into());
    }
    let g = &ir.scene_json["ground"];
    let num = |v: &serde_json::Value| v.as_f64().map(|x| x as f32).ok_or("scene.json: ground");
    let (v0, v1) = (num(&g["v"][0])?, num(&g["v"][1])?);
    let page = ((v0.min(v1) * pages as f32) as u32).min(pages - 1);
    if ((v0.max(v1) * pages as f32 - 1e-4) as u32).min(pages - 1) != page {
        return Err("the ground's texture strip crosses two atlas pages".into());
    }
    let local = |v: f32| (v * pages as f32 - page as f32).clamp(0.0, 1.0);
    let head = GroundHeader { n: n as u32, cell: field.cell, min: field.min, u_per_square: num(&g["uPerSquare"])?, v: [local(v0), local(v1)], page, pad: 0 };
    let mut out = pack::bytes_of(&head).to_vec();
    out.extend_from_slice(pack::slice_bytes(&grid));
    Ok(out)
}

/// Sections of geometry: resident vertices and indices, the on-demand blob, the clip bytes, the table.
pub struct Geometry {
    pub recs: Vec<HandMesh>,
    pub vtx: Vec<u8>,
    pub idx: Vec<u16>,
    pub near: Vec<u8>,
    pub clip: Vec<u8>,
    pub tris: [usize; 4],
    pub count: [usize; 4],
    pub big: usize,
    pub max_verts: usize,
    pub largest_cell: usize,
}

/// 3DS: meshes of one level, one atlas page and one block of cells share a
/// vertex base, and their indices follow each other. Every mesh is on the same
/// position grid, so the runtime draws a run of adjacent visible meshes with
/// one call instead of one call each.
pub fn assemble_grouped(lowered: Vec<Vec<Lowered>>, vertex_bytes: usize, block_cells: i32) -> Geometry {
    let mut g = Geometry { recs: Vec::new(), vtx: Vec::new(), idx: Vec::new(), near: Vec::new(), clip: Vec::new(), tris: [0; 4], count: [0; 4], big: 0, max_verts: 0, largest_cell: 0 };
    let mut all: Vec<Lowered> = lowered.into_iter().flatten().collect();
    let key = |m: &Lowered| (m.rec.kind, m.rec.page, m.rec.cz.div_euclid(block_cells), m.rec.cx.div_euclid(block_cells), m.rec.cz, m.rec.cx);
    all.sort_by_key(key);
    let mut group = (u32::MAX, u32::MAX, i32::MAX, i32::MAX);
    let (mut base, mut used) = (0usize, 0usize);
    for m in all {
        let k = key(&m);
        let n = m.vtx.len() / vertex_bytes;
        if (k.0, k.1, k.2, k.3) != group || used + n > 65536 {
            group = (k.0, k.1, k.2, k.3);
            base = g.vtx.len() / vertex_bytes;
            used = 0;
        }
        let mut rec = m.rec;
        rec.vtx_first = base as u32;
        rec.idx_first = g.idx.len() as u32;
        g.idx.extend(m.idx.iter().map(|&i| i + used as u16));
        g.vtx.extend_from_slice(&m.vtx);
        used += n;
        g.tris[rec.kind as usize] += m.idx.len() / 3;
        g.count[rec.kind as usize] += 1;
        g.max_verts = g.max_verts.max(used);
        g.recs.push(rec);
    }
    while g.idx.len() % 2 != 0 {
        g.idx.push(0);
    }
    g
}

pub fn assemble(lowered: Vec<Vec<Lowered>>, stream_near: bool, vertex_bytes: usize) -> Geometry {
    let mut g = Geometry { recs: Vec::new(), vtx: Vec::new(), idx: Vec::new(), near: Vec::new(), clip: Vec::new(), tris: [0; 4], count: [0; 4], big: 0, max_verts: 0, largest_cell: 0 };
    for group in lowered {
        // One group is one cell at one level; streamed, its meshes are contiguous: vertices, then indices.
        let streamed = stream_near && group.first().is_some_and(|m| m.rec.kind == mesh_kind::NEAR);
        let start = g.near.len();
        let mut recs: Vec<HandMesh> = Vec::new();
        for m in &group {
            let mut rec = m.rec;
            rec.clip_first = g.clip.len() as u32;
            g.clip.extend_from_slice(&m.clip);
            if streamed {
                rec.vtx_first = g.near.len() as u32;
                g.near.extend_from_slice(&m.vtx);
                while g.near.len() % 4 != 0 {
                    g.near.push(0);
                }
            } else {
                rec.vtx_first = (g.vtx.len() / vertex_bytes) as u32;
                g.vtx.extend_from_slice(&m.vtx);
                rec.idx_first = g.idx.len() as u32;
                g.idx.extend_from_slice(&m.idx);
            }
            g.tris[rec.kind as usize] += m.idx.len() / 3;
            g.count[rec.kind as usize] += 1;
            g.big += m.big;
            g.max_verts = g.max_verts.max(rec.vtx_count as usize);
            recs.push(rec);
        }
        if streamed {
            for (m, rec) in group.iter().zip(&mut recs) {
                rec.idx_first = g.near.len() as u32;
                g.near.extend_from_slice(pack::slice_bytes(&m.idx));
                while g.near.len() % 4 != 0 {
                    g.near.push(0);
                }
            }
            g.largest_cell = g.largest_cell.max(g.near.len() - start);
        }
        g.recs.extend(recs);
    }
    while g.idx.len() % 2 != 0 {
        g.idx.push(0);
    }
    g
}

// ---------------------------------------------------------------- the map

/// `MAPT` is `width`, `height`, the half-extent in metres as f32, a zero, then the texels.
/// The field from above for the 3DS's lower screen: `size` × `size` RGB565, row 0 at the north edge, covering
/// `±extent` metres. Heights shade it; the cohorts' muster is marked.
pub fn map(field: &requiem_sim::field::Field, stage: &requiem_sim::worldfile::Stage, sun: [f32; 3], size: usize, extent: f32) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + size * size * 2);
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&extent.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    let at = |i: usize| -extent + (i as f32 + 0.5) / size as f32 * 2.0 * extent;
    let mut rgb = vec![[0.0f32; 3]; size * size];
    for j in 0..size {
        for i in 0..size {
            let (x, z) = (at(i), at(j));
            let n = field.normal(x, z);
            let h = field.height(x, z);
            let lit = (n.x * sun[0] + n.y * sun[1] + n.z * sun[2]).max(0.0);
            let k = 0.35 + 0.65 * lit;
            let high = ((h + 5.0) / 70.0).clamp(0.0, 1.0);
            rgb[j * size + i] = [(0.10 + 0.10 * high) * k, (0.16 + 0.10 * high) * k, (0.30 + 0.12 * high) * k];
        }
    }
    for m in &stage.musters {
        let (i, j) = (((m.x + extent) / (2.0 * extent) * size as f32) as isize, ((m.z + extent) / (2.0 * extent) * size as f32) as isize);
        for (di, dj) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let (x, y) = (i + di, j + dj);
            if x >= 0 && y >= 0 && (x as usize) < size && (y as usize) < size {
                rgb[y as usize * size + x as usize] = [0.42, 0.46, 0.6];
            }
        }
    }
    for c in rgb {
        let q = |v: f32, bits: u32| ((v.clamp(0.0, 1.0).powf(1.0 / 2.2) * ((1 << bits) - 1) as f32) + 0.5) as u16;
        out.extend_from_slice(&((q(c[0], 5) << 11) | (q(c[1], 6) << 5) | q(c[2], 5)).to_le_bytes());
    }
    out
}
