//! lower-effects: the effects' templates and constants in the device's layout.
//!
//! The reference lowers each layer of each effect to a template of vertices
//! (three vectors with every component in [-1, 1]) and eight rows of
//! constants. This pass stores a template vertex as twelve signed bytes and
//! lays the layers out for one read.

use requiem_pack::{self as pack, FxEffect, FxHeader, FxLayer, FxVertex};
use serde_json::{json, Value};

fn pad16(out: &mut Vec<u8>) {
    while out.len() % 16 != 0 {
        out.push(0);
    }
}

/// `keep_atlas`: the atlas's bytes go into the section (the Vita reads them there); without, a device reads `FXTX`.
pub fn lower(src: &[u8], keep_atlas: bool) -> Result<(Vec<u8>, Value), String> {
    let word = |at: usize| src.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or("fx.bin is truncated".to_string());
    if word(0)? != u32::from_le_bytes(*b"RQFX") || word(4)? != 1 {
        return Err("fx.bin has the wrong magic or version".into());
    }
    let (count, atlas) = (word(8)? as usize, word(12)? as usize);
    let atlas_bytes = src.get(16..16 + atlas * atlas).ok_or("fx.bin is truncated")?;
    let mut at = 16 + atlas * atlas;
    let mut effects = Vec::with_capacity(count);
    let mut layers: Vec<FxLayer> = Vec::new();
    let mut blobs: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for _ in 0..count {
        let n = word(at)? as usize;
        at += 4;
        effects.push(FxEffect { first: layers.len() as u32, count: n as u32 });
        for _ in 0..n {
            let (program, blend, nv, ni) = (word(at)?, word(at + 4)?, word(at + 8)? as usize, word(at + 12)? as usize);
            at += 16;
            let mut rows = [0.0f32; 32];
            for (k, r) in rows.iter_mut().enumerate() {
                *r = f32::from_bits(word(at + k * 4)?);
            }
            at += 128;
            let floats = src.get(at..at + nv * 48).ok_or("fx.bin is truncated")?;
            at += nv * 48;
            let mut verts = Vec::with_capacity(nv * 12);
            for c in floats.chunks_exact(4) {
                let v = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                if !(-1.0001..=1.0001).contains(&v) {
                    return Err(format!("an effect template holds {v}; components are stored over [-1, 1]"));
                }
                verts.push((v * 127.0).round() as i8 as u8);
            }
            let idx = src.get(at..at + ni * 2).ok_or("fx.bin is truncated")?.to_vec();
            at += (ni * 2 + 3) & !3;
            if nv > 65535 {
                return Err("an effect template has more than 65535 vertices".into());
            }
            layers.push(FxLayer { program, blend, vtx_count: nv as u32, idx_count: ni as u32, vtx_at: 0, idx_at: 0, pad: [0; 2], rows });
            blobs.push((verts, idx));
        }
    }
    let head = core::mem::size_of::<FxHeader>() + effects.len() * core::mem::size_of::<FxEffect>() + layers.len() * core::mem::size_of::<FxLayer>();
    let mut data = vec![0u8; head];
    pad16(&mut data);
    let atlas_at = data.len() as u32;
    if keep_atlas {
        data.extend_from_slice(atlas_bytes);
    }
    pad16(&mut data);
    let (mut vertices, mut triangles) = (0usize, 0usize);
    for (l, (verts, idx)) in layers.iter_mut().zip(&blobs) {
        l.vtx_at = data.len() as u32;
        data.extend_from_slice(verts);
        pad16(&mut data);
        l.idx_at = data.len() as u32;
        data.extend_from_slice(idx);
        pad16(&mut data);
        vertices += l.vtx_count as usize;
        triangles += l.idx_count as usize / 3;
    }
    debug_assert_eq!(core::mem::size_of::<FxVertex>(), 12);
    let header = FxHeader { effects: effects.len() as u32, layers: layers.len() as u32, atlas: if keep_atlas { atlas as u32 } else { 0 }, atlas_at };
    let mut o = 0;
    let mut put = |bytes: &[u8]| {
        data[o..o + bytes.len()].copy_from_slice(bytes);
        o += bytes.len();
    };
    put(pack::bytes_of(&header));
    put(pack::slice_bytes(&effects));
    put(pack::slice_bytes(&layers));
    Ok((data, json!({"effects": effects.iter().filter(|e| e.count > 0).count(), "layers": layers.len(), "templateVertices": vertices, "templateTriangles": triangles, "atlas": atlas})))
}
