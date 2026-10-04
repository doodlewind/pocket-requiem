//! bake-crowd: the army's motion compiled out of the runtime.
//!
//! The simulation defines each kind of knight's clips as functions from time
//! to a pose (`requiem_sim::knight`). This pass samples every clip at its
//! stored frame times, skins each level of detail of the kind's model at each
//! frame, and writes the placed vertices. On the device a knight has no
//! skeleton: it is a blend of two of these frames.

use crate::ir::{Ir, Mesh, SKIN_STRIDE};
use rayon::prelude::*;
use requiem_pack::{self as pack, CrowdHeader, CrowdMesh, CrowdVertex};
use requiem_sim::anim::skin;
use requiem_sim::knight::{Knight, FRAMES};
use requiem_sim::math::*;
use serde_json::{json, Value};

/// Metres a position's full 16-bit range stands for: a raised halberd's point is 3.4 m from the figure's origin.
const SCALE: f32 = 4.0;
const KINDS: u32 = 3;

fn pad16(out: &mut Vec<u8>) {
    while out.len() % 16 != 0 {
        out.push(0);
    }
}

/// How bare the metal is where the tint is `c`: plate is light and grey, cloth and leather are dark or warm.
pub fn metal(c: &[f32]) -> u8 {
    let (hi, lo) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
    let grey = 1.0 - ((hi - lo) / hi.max(1e-3) * 4.0).clamp(0.0, 1.0);
    let bright = ((hi - 0.25) / 0.25).clamp(0.0, 1.0);
    (grey * bright * 255.0 + 0.5) as u8
}

fn frames(model: &Mesh, knight: &Knight) -> Result<Vec<CrowdVertex>, String> {
    const S: usize = SKIN_STRIDE;
    let nv = model.verts.len() / S;
    let bind_inv = knight.skel.bind_inverse();
    let per: Vec<Result<Vec<CrowdVertex>, String>> = (0..FRAMES as u16)
        .into_par_iter()
        .map(|f| {
            let mats = skin(&knight.frame(f), &bind_inv);
            let mut out = Vec::with_capacity(nv);
            for i in 0..nv {
                let v = &model.verts[i * S..(i + 1) * S];
                let (p, n) = (v3(v[0], v[1], v[2]), v3(v[3], v[4], v[5]));
                let (a, b, wa) = (v[9] as usize, v[10] as usize, v[11]);
                let pos = mats[a].apply(p) * wa + mats[b].apply(p) * (1.0 - wa);
                let nrm = (mats[a].r.apply(n) * wa + mats[b].r.apply(n) * (1.0 - wa)).norm_or(V3::UP);
                let q = |x: f32| -> Result<i16, String> {
                    if x.abs() > SCALE {
                        return Err(format!("a knight's vertex is {x:.2} m from its origin in frame {f}; positions hold {SCALE} m"));
                    }
                    Ok((x / SCALE * 32767.0).round() as i16)
                };
                let s = |x: f32| (x.clamp(-1.0, 1.0) * 127.0).round() as i8;
                out.push(CrowdVertex { pos: [q(pos.x)?, q(pos.y)?, q(pos.z)?], pad: 0, normal: [s(nrm.x), s(nrm.y), s(nrm.z), 0] });
            }
            Ok(out)
        })
        .collect();
    let mut all = Vec::with_capacity(nv * FRAMES);
    for f in per {
        all.extend(f?);
    }
    Ok(all)
}

/// The `CRWD` section for the levels of detail the profile names, and its statistics.
pub fn bake(ir: &Ir, lods: &[u32]) -> Result<(Vec<u8>, Value), String> {
    const S: usize = SKIN_STRIDE;
    let head = core::mem::size_of::<CrowdHeader>() + KINDS as usize * lods.len() * core::mem::size_of::<CrowdMesh>();
    let mut data: Vec<u8> = vec![0; head];
    pad16(&mut data);
    let mut recs = Vec::new();
    let mut stats = Vec::new();
    for kind in 1..=KINDS {
        let knight = Knight::new(kind);
        for (level, &source) in lods.iter().enumerate() {
            let id = (kind * 100 + source) as i32;
            let model = ir.models.iter().find(|m| m.head[0] == id).ok_or(format!("the source has no knight model {id}"))?;
            let nv = model.verts.len() / S;
            if nv > 65535 {
                return Err(format!("knight model {id} has {nv} vertices; indices are 16-bit"));
            }
            let color_at = data.len() as u32;
            for i in 0..nv {
                let c = &model.verts[i * S + 6..i * S + 9];
                let b = |x: f32| (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                data.extend_from_slice(&[b(c[0]), b(c[1]), b(c[2]), metal(c)]);
            }
            pad16(&mut data);
            let idx_at = data.len() as u32;
            for &i in &model.idx {
                data.extend_from_slice(&(i as u16).to_le_bytes());
            }
            pad16(&mut data);
            let frames_at = data.len() as u32;
            data.extend_from_slice(pack::slice_bytes(&frames(model, &knight)?));
            pad16(&mut data);
            recs.push(CrowdMesh { kind: kind - 1, lod: level as u32, vtx_count: nv as u32, idx_count: model.idx.len() as u32, color_at, idx_at, frames_at, pad: 0 });
            stats.push(json!({"kind": kind, "lod": level, "source": source, "vertices": nv, "triangles": model.idx.len() / 3, "frameBytes": nv * FRAMES * core::mem::size_of::<CrowdVertex>()}));
        }
    }
    let header = CrowdHeader { kinds: KINDS, lods: lods.len() as u32, frames: FRAMES as u32, scale: SCALE };
    let mut at = 0;
    data[at..at + core::mem::size_of::<CrowdHeader>()].copy_from_slice(pack::bytes_of(&header));
    at += core::mem::size_of::<CrowdHeader>();
    for r in &recs {
        data[at..at + core::mem::size_of::<CrowdMesh>()].copy_from_slice(pack::bytes_of(r));
        at += core::mem::size_of::<CrowdMesh>();
    }
    Ok((data, json!({"frames": FRAMES, "meshes": stats})))
}
