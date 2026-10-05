//! Stage compiler: StageIR in, device pack out.
//!
//! Passes, in order (`RECIPE`): read-source, bake-lighting, merge-cells,
//! quantize, bake-crowd, lower-effects, atlas-mips, interface-font, structural-budgets. The
//! compile receipt next to the pack records the source, the profile, every
//! section's size and hash, and the statistics a frame budget is argued from.
//!
//! A profile's `target` picks the lowering: `vita` (this file), or `psp` and
//! `3ds` (`handheld.rs`), which store the ground as grids and cut the models,
//! the army and the textures to what each machine reads.
//!
//! `requiem-cook --in <StageIR dir> --out <pack> --profile <json> --font <ttf>`

mod bake;
mod crowd;
mod font;
mod fx;
mod handheld;
mod ir;
mod texture;

use ir::{layer, Mesh, STRIDE};
use requiem_pack::{self as pack, mesh_kind, MeshRec, ModelHeader, SkinVertex, Vertex};
use rayon::prelude::*;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

const RECIPE: &[(&str, u32)] = &[("read-source", 1), ("bake-lighting", 1), ("merge-cells", 1), ("quantize", 1), ("bake-crowd", 1), ("lower-effects", 1), ("atlas-mips", 1), ("interface-font", 1), ("structural-budgets", 1)];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Profile {
    name: String,
    target: String,
    presentation: Presentation,
    texture: TextureProfile,
    bake: BakeProfile,
    limits: Limits,
    font: FontProfile,
    /// Vita.
    #[serde(default)]
    crowd: Option<CrowdProfile>,
    /// PSP and 3DS.
    #[serde(default)]
    handheld: Option<handheld::Handheld>,
}
/// Which of the exported levels of detail of a knight the pack carries, nearest first,
/// and the distance in metres at which each hands over to the next; the last is where a knight is no longer drawn.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CrowdProfile {
    lods: Vec<u32>,
    reach: Vec<f32>,
    /// Triangles of knights a frame may draw; past it the runtime pulls the hand-over distances in.
    budget: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Presentation {
    render: [u32; 2],
    display: [u32; 2],
    target_fps: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TextureProfile {
    format: String,
    max_mips: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BakeProfile {
    sun_rays: u32,
    sun_spread: f32,
    ao_rays: u32,
    ao_reach: f32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Limits {
    max_mesh_vertices: usize,
    max_pack_bytes: usize,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FontProfile {
    sizes: Vec<u32>,
    atlas: [usize; 2],
}

struct OutMesh {
    kind: u32,
    cx: i32,
    cz: i32,
    verts: Vec<Vertex>,
    idx: Vec<u16>,
    min: [f32; 3],
    max: [f32; 3],
}

/// Merges baked source meshes into device meshes of at most `limit` vertices.
fn lower(parts: &[(&Mesh, &Vec<[u8; 4]>)], kind: u32, cx: i32, cz: i32, limit: usize) -> Result<Vec<OutMesh>, String> {
    // Chunk first (source vertex, colour), then quantize each chunk over its own bounds.
    struct Chunk<'a> {
        src: Vec<(&'a [f32], [u8; 4])>,
        idx: Vec<u16>,
    }
    let mut chunks = vec![Chunk { src: Vec::new(), idx: Vec::new() }];
    for (mesh, colors) in parts {
        let mut map = vec![u32::MAX; mesh.verts.len() / STRIDE];
        for tri in mesh.idx.chunks_exact(3) {
            let fresh = tri.iter().filter(|&&i| map[i as usize] == u32::MAX).count();
            if chunks.last().unwrap().src.len() + fresh > limit {
                chunks.push(Chunk { src: Vec::new(), idx: Vec::new() });
                map.iter_mut().for_each(|m| *m = u32::MAX);
            }
            let c = chunks.last_mut().unwrap();
            for &i in tri {
                let i = i as usize;
                if map[i] == u32::MAX {
                    map[i] = c.src.len() as u32;
                    c.src.push((&mesh.verts[i * STRIDE..(i + 1) * STRIDE], colors[i]));
                }
                c.idx.push(map[i] as u16);
            }
        }
    }
    let mut out = Vec::new();
    for c in chunks.into_iter().filter(|c| !c.idx.is_empty()) {
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        for (v, _) in &c.src {
            for a in 0..3 {
                min[a] = min[a].min(v[a]);
                max[a] = max[a].max(v[a]);
            }
        }
        for a in 0..3 {
            if max[a] - min[a] < 0.01 {
                max[a] = min[a] + 0.01;
            }
        }
        let mut verts = Vec::with_capacity(c.src.len());
        for (v, color) in &c.src {
            let mut pos = [0u16; 3];
            for a in 0..3 {
                pos[a] = ((v[a] - min[a]) / (max[a] - min[a]) * 65535.0 + 0.5).clamp(0.0, 65535.0) as u16;
            }
            let u = v[6] / pack::UV_SCALE;
            if u.abs() > 1.0 || v[7] < 0.0 || v[7] > 1.0 {
                return Err(format!("texture coordinate ({}, {}) is outside the quantized range in cell {cx},{cz}", v[6], v[7]));
            }
            verts.push(Vertex { pos, pad: 0, uv: [(u * 32767.0).round() as i16, (v[7] * 32767.0).round() as i16], color: *color });
        }
        out.push(OutMesh { kind, cx, cz, verts, idx: c.idx, min, max });
    }
    Ok(out)
}

fn models(ir: &ir::Ir) -> Result<Vec<u8>, String> {
    const S: usize = ir::SKIN_STRIDE;
    // The mage and the demon at full resolution. The knights go through `crowd::bake`.
    let mine: Vec<&Mesh> = ir.models.iter().filter(|m| m.head[0] == 0 || m.head[0] == 4).collect();
    let mut out = (mine.len() as u32).to_le_bytes().to_vec();
    for m in mine {
        let nv = m.verts.len() / S;
        if nv > 65535 {
            return Err(format!("model {} has {nv} vertices; indices are 16-bit", m.head[0]));
        }
        let head = ModelHeader { id: m.head[0] as u32, vtx_count: nv as u32, idx_count: m.idx.len() as u32, pad: 0 };
        out.extend_from_slice(pack::bytes_of(&head));
        for i in 0..nv {
            let v = &m.verts[i * S..(i + 1) * S];
            let c = |x: f32| (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            let n = |x: f32| (x.clamp(-1.0, 1.0) * 127.0).round() as i8;
            let w0 = (v[11].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            let sv = SkinVertex { pos: [v[0], v[1], v[2]], normal: [n(v[3]), n(v[4]), n(v[5]), 0], color: [c(v[6]), c(v[7]), c(v[8]), 255], bones: [v[9] as u8, v[10] as u8], weights: [w0, 255 - w0] };
            out.extend_from_slice(pack::bytes_of(&sv));
        }
        for &i in &m.idx {
            out.extend_from_slice(&(i as u16).to_le_bytes());
        }
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    Ok(out)
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

/// What every target's lowering starts from.
struct Cooked<'a> {
    profile: &'a Profile,
    profile_bytes: &'a [u8],
    ir: &'a ir::Ir,
    field: &'a requiem_sim::field::Field,
    stage: &'a requiem_sim::worldfile::Stage,
    /// Baked colour per vertex of each bucket.
    colors: &'a [Vec<[u8; 4]>],
    output: &'a PathBuf,
    font_path: &'a PathBuf,
    bake_ms: u128,
    t_all: Instant,
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let input = PathBuf::from(arg(&args, "--in").ok_or("--in <WorldIR directory> is required")?);
    let output = PathBuf::from(arg(&args, "--out").ok_or("--out <pack> is required")?);
    let profile_path = PathBuf::from(arg(&args, "--profile").ok_or("--profile <json> is required")?);
    let font_path = PathBuf::from(arg(&args, "--font").ok_or("--font <ttf> is required")?);

    let t_all = Instant::now();
    let profile_bytes = std::fs::read(&profile_path).map_err(|e| format!("{}: {e}", profile_path.display()))?;
    let profile: Profile = serde_json::from_slice(&profile_bytes).map_err(|e| format!("{}: {e}", profile_path.display()))?;
    let ir = ir::load(&input)?;
    let (field, stage) = requiem_sim::worldfile::parse(&ir.world).map_err(|e| format!("stage.rqsw: {e}"))?;

    // bake-lighting
    let t = Instant::now();
    let settings = bake::Settings { sun_rays: profile.bake.sun_rays, sun_spread: profile.bake.sun_spread, ao_rays: profile.bake.ao_rays, ao_reach: profile.bake.ao_reach };
    let colors: Vec<Vec<[u8; 4]>> = ir.buckets.par_iter().map(|m| bake::bake(m, &ir.scene, &ir.collision, &settings)).collect();
    let bake_ms = t.elapsed().as_millis();
    let c = Cooked { profile: &profile, profile_bytes: &profile_bytes, ir: &ir, field: &field, stage: &stage, colors: &colors, output: &output, font_path: &font_path, bake_ms, t_all };
    match (profile.target.as_str(), &profile.handheld, &profile.crowd) {
        ("vita", None, Some(crowd)) if profile.texture.format == "bc1" => cook_vita(c, crowd),
        ("psp", Some(h), None) => cook_handheld(c, handheld::Target::Psp, h),
        ("3ds", Some(h), None) => cook_handheld(c, handheld::Target::Pica, h),
        _ => Err(format!("profile {}: target {} needs `crowd` and bc1 textures (vita) or `handheld` (psp, 3ds)", profile.name, profile.target)),
    }
}

fn publish(c: &Cooked, bytes: &[u8], sections: &[(u32, &[u8])], stats: &serde_json::Value, timings: serde_json::Value) -> Result<(), String> {
    if bytes.len() > c.profile.limits.max_pack_bytes {
        return Err(format!("the pack is {} bytes; the profile allows {}", bytes.len(), c.profile.limits.max_pack_bytes));
    }
    pack::Pack::parse(bytes)?;
    // The pack, then the receipt that names it.
    let temp = c.output.with_extension("tmp");
    std::fs::write(&temp, bytes).map_err(|e| format!("{}: {e}", temp.display()))?;
    std::fs::rename(&temp, c.output).map_err(|e| format!("{}: {e}", c.output.display()))?;
    let receipt = json!({
        "compiler": {"name": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION"), "packVersion": pack::VERSION},
        "recipe": RECIPE.iter().map(|(n, v)| json!({"pass": n, "version": v})).collect::<Vec<_>>(),
        "source": {"name": c.ir.manifest.name, "seed": c.ir.manifest.seed, "manifestSha256": c.ir.manifest_sha256},
        "profile": {"name": c.profile.name, "sha256": ir::sha256(c.profile_bytes)},
        "artifact": {"path": c.output.file_name().map(|s| s.to_string_lossy()), "bytes": bytes.len(), "sha256": ir::sha256(bytes)},
        "sections": sections.iter().map(|(t, d)| json!({"tag": String::from_utf8_lossy(&t.to_le_bytes()), "bytes": d.len(), "sha256": ir::sha256(d)})).collect::<Vec<_>>(),
        "stats": stats,
        "frameBudget": {"fps": c.profile.presentation.target_fps, "milliseconds": 1000.0 / c.profile.presentation.target_fps as f64},
        "timingsMs": timings,
    });
    let receipt_path = PathBuf::from(format!("{}.compile.json", c.output.display()));
    std::fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", receipt_path.display()))
}

fn cook_vita(c: Cooked, crowd: &CrowdProfile) -> Result<(), String> {
    let (profile, ir, colors) = (c.profile, c.ir, c.colors);
    // merge-cells + quantize: a cell's near mesh is its ground at 4 m and its near props, its middle mesh its ground at 8 m
    // and its middle props; a super-cell's far mesh is its ground at 32 m and its far props.
    let t = Instant::now();
    let mut cells: BTreeMap<(i32, i32), [Vec<usize>; 5]> = BTreeMap::new();
    let mut supers: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
    let mut plan: Vec<(u32, i32, i32, Vec<usize>)> = Vec::new();
    for (i, b) in ir.buckets.iter().enumerate() {
        let slot = match b.head[0] {
            layer::BASE => 0,
            layer::NEAR => 1,
            layer::MID => 2,
            layer::GROUND_NEAR => 3,
            layer::GROUND_MID => 4,
            layer::FAR | layer::GROUND_FAR => {
                supers.entry((b.head[2], b.head[1])).or_default().push(i);
                continue;
            }
            layer::BACKDROP => {
                plan.push((mesh_kind::BACKDROP, 0, 0, vec![i]));
                continue;
            }
            l => return Err(format!("unknown layer {l}")),
        };
        cells.entry((b.head[2], b.head[1])).or_default()[slot].push(i);
    }
    for ((cz, cx), [base, near, mid, ground_near, ground_mid]) in &cells {
        plan.push((mesh_kind::NEAR, *cx, *cz, ground_near.iter().chain(base).chain(near).copied().collect()));
        plan.push((mesh_kind::MID, *cx, *cz, ground_mid.iter().chain(base).chain(mid).copied().collect()));
    }
    for ((cz, cx), mut parts) in supers {
        // The ground first.
        parts.sort_by_key(|&i| ir.buckets[i].head[0] != layer::GROUND_FAR);
        plan.push((mesh_kind::FAR, cx, cz, parts));
    }
    plan.sort_by_key(|p| (p.0, p.2, p.1));
    let limit = profile.limits.max_mesh_vertices;
    let lowered: Vec<Vec<OutMesh>> = plan
        .par_iter()
        .map(|(kind, cx, cz, parts)| {
            let parts: Vec<(&Mesh, &Vec<[u8; 4]>)> = parts.iter().map(|&i| (&ir.buckets[i], &colors[i])).collect();
            lower(&parts, *kind, *cx, *cz, limit)
        })
        .collect::<Result<_, _>>()?;
    let mut recs = Vec::new();
    let mut vtx = Vec::new();
    let mut idx = Vec::new();
    let mut tris = [0usize; 4];
    let mut count = [0usize; 4];
    let mut max_verts = 0;
    for m in lowered.into_iter().flatten() {
        recs.push(MeshRec { kind: m.kind, cx: m.cx, cz: m.cz, vtx_first: vtx.len() as u32, vtx_count: m.verts.len() as u32, idx_first: idx.len() as u32, idx_count: m.idx.len() as u32, min: m.min, max: m.max, pad: 0 });
        tris[m.kind as usize] += m.idx.len() / 3;
        count[m.kind as usize] += 1;
        max_verts = max_verts.max(m.verts.len());
        vtx.extend_from_slice(&m.verts);
        idx.extend_from_slice(&m.idx);
    }
    while idx.len() % 2 != 0 {
        idx.push(0);
    }
    let lower_ms = t.elapsed().as_millis();

    // bake-crowd: every kind of knight, at the profile's levels of detail, placed at every stored frame
    let t = Instant::now();
    let (crowd_bytes, crowd_stats) = crowd::bake(ir, &crowd.lods)?;
    let crowd_ms = t.elapsed().as_millis();
    let (fx_bytes, fx_stats) = fx::lower(&ir.fx, true)?;

    // atlas-mips-bc1, interface-font
    let t = Instant::now();
    let (tex, mips) = texture::atlas(&ir.atlas, ir.scene.atlas.width, ir.scene.atlas.height, &ir.scene.atlas.strip_edges, profile.texture.max_mips);
    let ttf = std::fs::read(c.font_path).map_err(|e| format!("{}: {e}", c.font_path.display()))?;
    let font = font::bake(&ttf, &profile.font.sizes, profile.font.atlas[0], profile.font.atlas[1])?;
    let tex_ms = t.elapsed().as_millis();

    let stats = json!({
        "meshes": {"near": count[0], "mid": count[1], "far": count[2], "backdrop": count[3]},
        "triangles": {"near": tris[0], "mid": tris[1], "far": tris[2], "backdrop": tris[3]},
        "vertices": vtx.len(),
        "indices": idx.len(),
        "maxMeshVertices": max_verts,
        "bakeTriangles": ir.collision_triangles,
        "atlasMips": mips,
        "crowd": crowd_stats,
        "effects": fx_stats,
        "stage": {"knights": c.stage.musters.iter().map(|m| m.cols as u32 * m.rows as u32).sum::<u32>(), "cohorts": c.stage.musters.len(), "obstacles": c.field.obstacles.len(), "heights": c.field.n},
    });
    let mut scene = ir.scene_json.clone();
    if let Some(o) = scene.as_object_mut() {
        // The runtime computes the sky from the same constants.
        o.remove("skyTable");
        o.remove("source");
    }
    let meta = json!({
        "name": ir.manifest.name,
        "seed": ir.manifest.seed,
        "source": ir.manifest_sha256,
        "profile": profile.name,
        "presentation": {"render": profile.presentation.render, "display": profile.presentation.display, "targetFps": profile.presentation.target_fps},
        "scene": scene,
        "crowd": {"reach": crowd.reach, "budget": crowd.budget},
        "stats": stats,
    });
    let meta_bytes = serde_json::to_vec(&meta).map_err(|e| e.to_string())?;
    let model_bytes = models(ir)?;
    let sections: Vec<(u32, &[u8])> = vec![
        (pack::META, &meta_bytes),
        (pack::TEX0, &tex),
        (pack::MESH, pack::slice_bytes(&recs)),
        (pack::VTX0, pack::slice_bytes(&vtx)),
        (pack::IDX0, pack::slice_bytes(&idx)),
        (pack::MODL, &model_bytes),
        (pack::CRWD, &crowd_bytes),
        (pack::FXPK, &fx_bytes),
        (pack::FONT, &font),
        (pack::SIMW, &ir.world),
    ];
    let bytes = pack::write(&sections);

    // structural-budgets
    if crowd.reach.len() != crowd.lods.len() {
        return Err("the profile's crowd needs one reach per level of detail".into());
    }
    if max_verts > limit {
        return Err(format!("a mesh has {max_verts} vertices; the profile allows {limit}"));
    }
    publish(&c, &bytes, &sections, &stats, json!({"bake": c.bake_ms, "lower": lower_ms, "crowd": crowd_ms, "textures": tex_ms, "total": c.t_all.elapsed().as_millis()}))?;
    println!("{}: {:.1} MB, {} meshes, {} vertices; bake {} ms, lower {lower_ms} ms, textures {tex_ms} ms", c.output.display(), bytes.len() as f64 / 1e6, recs.len(), vtx.len(), c.bake_ms);
    println!("triangles: near {} mid {} far {} backdrop {}; largest mesh {} vertices", tris[0], tris[1], tris[2], tris[3], max_verts);
    println!("crowd: {:.1} MB of frames; {crowd_stats}", crowd_bytes.len() as f64 / 1e6);
    Ok(())
}

/// PSP and 3DS. The ground leaves as two grids (the heights are in the simulation's world file, the baked
/// colours in `GRND`) and the device builds its meshes; a cell's props are the source's middle props, and the
/// far props are stored twice: cut by cell, for a super-cell that is near enough to draw cell by cell, and whole.
fn cook_handheld(c: Cooked, target: handheld::Target, h: &handheld::Handheld) -> Result<(), String> {
    let (profile, ir, colors) = (c.profile, c.ir, c.colors);
    let format = handheld::format_of(&profile.texture.format)?;
    let psp = target == handheld::Target::Psp;
    if psp != matches!(format, pack::tex_format::PSP_DXT1 | pack::tex_format::PSP_5650 | pack::tex_format::PSP_T8) {
        return Err(format!("profile {}: texture format {} is not one of target {}", profile.name, profile.texture.format, profile.target));
    }

    // atlas-mips, interface-font
    let t = Instant::now();
    let (tex, pages, mips) = handheld::atlas(&ir.atlas, ir.scene.atlas.width, ir.scene.atlas.height, &ir.scene.atlas.strip_edges, h.page, format, profile.texture.max_mips)?;
    let ttf = std::fs::read(c.font_path).map_err(|e| format!("{}: {e}", c.font_path.display()))?;
    let font = handheld::font(&font::bake(&ttf, &profile.font.sizes, profile.font.atlas[0], profile.font.atlas[1])?, target)?;
    let tex_ms = t.elapsed().as_millis();

    // merge-cells + quantize
    let t = Instant::now();
    let cell = ir.scene_json["cell"].as_f64().ok_or("scene.json: cell")? as f32;
    let near_layer = match h.props.as_str() {
        "near" => layer::NEAR,
        "mid" => layer::MID,
        other => return Err(format!("profile {}: props {other:?} is not near or mid", profile.name)),
    };
    // The far props cut by cell.
    let cut: Vec<(Mesh, Vec<[u8; 4]>)> = ir.buckets.iter().zip(colors).filter(|(b, _)| b.head[0] == layer::FAR).flat_map(|(b, col)| handheld::cut_by_cell(b, col, cell)).collect();
    let mut plan: Vec<(u32, i32, i32, Vec<(&Mesh, &Vec<[u8; 4]>)>)> = Vec::new();
    for (b, col) in ir.buckets.iter().zip(colors) {
        match b.head[0] {
            l if l == near_layer || l == layer::BASE => plan.push((mesh_kind::NEAR, b.head[1], b.head[2], vec![(b, col)])),
            layer::FAR => plan.push((mesh_kind::FAR, b.head[1], b.head[2], vec![(b, col)])),
            layer::BACKDROP => plan.push((mesh_kind::BACKDROP, 0, 0, vec![(b, col)])),
            _ => {}
        }
    }
    for (m, col) in &cut {
        plan.push((mesh_kind::MID, m.head[1], m.head[2], vec![(m, col)]));
    }
    plan.sort_by_key(|p| (p.0, p.2, p.1));
    let limit = profile.limits.max_mesh_vertices;
    let lowered: Vec<Vec<handheld::Lowered>> = plan.par_iter().map(|(kind, cx, cz, parts)| handheld::lower(parts, *kind, *cx, *cz, h, pages, target, limit)).collect::<Result<_, _>>()?;
    let vertex_bytes = if psp { core::mem::size_of::<pack::PspVertex>() } else { core::mem::size_of::<pack::PicaVertex>() };
    // 3DS: blocks of 4 x 4 cells share a vertex base, so a run of visible meshes is one draw.
    let geo = if psp { handheld::assemble(lowered, false, vertex_bytes) } else { handheld::assemble_grouped(lowered, vertex_bytes, 4) };
    let ground = handheld::ground(ir, colors, c.field, pages)?;
    let lower_ms = t.elapsed().as_millis();

    // bake-crowd, lower-effects
    let t = Instant::now();
    let (crowd_tag, (crowd_bytes, crowd_stats)) = if psp { (pack::CRWP, handheld::crowd_psp(ir, &h.crowd.lods, &ir.scene)?) } else { (pack::CRWD, crowd::bake(ir, &h.crowd.lods)?) };
    let crowd_ms = t.elapsed().as_millis();
    let (fx_full, _) = fx::lower(&ir.fx, true)?;
    let fx_tex = handheld::fx_atlas(&fx_full, h.fx_atlas, target)?;
    let (fx_bytes, fx_stats) = fx::lower(&ir.fx, false)?;

    let (model_bytes, model_stats) = handheld::models(ir, &h.models, target)?;
    let scene = handheld::scene(&ir.scene_json, h, profile.presentation.render, pages)?;
    let knights = c.stage.musters.iter().map(|m| m.cols as u32 * m.rows as u32).sum::<u32>();

    let stats = json!({
        "meshes": {"near": geo.count[0], "mid": geo.count[1], "far": geo.count[2], "backdrop": geo.count[3]},
        "triangles": {"near": geo.tris[0], "mid": geo.tris[1], "far": geo.tris[2], "backdrop": geo.tris[3]},
        "largeTriangles": geo.big,
        "vertexBytes": geo.vtx.len(),
        "indices": geo.idx.len(),
        "maxMeshVertices": geo.max_verts,
        "groundGrid": c.field.n,
        "atlas": {"pages": pages, "page": h.page, "mips": mips, "format": profile.texture.format},
        "models": model_stats,
        "crowd": crowd_stats,
        "effects": fx_stats,
        "stage": {"knights": knights, "cohorts": c.stage.musters.len(), "obstacles": c.field.obstacles.len(), "heights": c.field.n},
    });
    let meta = json!({
        "name": ir.manifest.name,
        "seed": ir.manifest.seed,
        "source": ir.manifest_sha256,
        "profile": profile.name,
        "target": profile.target,
        "presentation": {"render": profile.presentation.render, "display": profile.presentation.display, "targetFps": profile.presentation.target_fps},
        "stats": stats,
    });
    let meta_bytes = serde_json::to_vec(&meta).map_err(|e| e.to_string())?;
    let mut sections: Vec<(u32, &[u8])> = vec![
        (pack::META, &meta_bytes),
        (pack::HSCN, pack::bytes_of(&scene)),
        (pack::TEX0, &tex),
        (pack::HMSH, pack::slice_bytes(&geo.recs)),
        (pack::VTX0, &geo.vtx),
        (pack::IDX0, pack::slice_bytes(&geo.idx)),
        (pack::CLIP, &geo.clip),
        (pack::GRND, &ground),
        (pack::MODL, &model_bytes),
        (crowd_tag, &crowd_bytes),
        (pack::FXPK, &fx_bytes),
        (pack::FXTX, &fx_tex),
        (pack::FONT, &font),
        (pack::SIMW, &ir.world),
    ];
    // The 3DS shows the field from above on its lower screen.
    let map = if psp { Vec::new() } else { handheld::map(c.field, c.stage, ir.scene.sun_dir, 240, c.field.half + 40.0) };
    if !psp {
        sections.push((pack::MAPT, &map));
    }
    let bytes = pack::write(&sections);
    if geo.max_verts > limit {
        return Err(format!("a mesh has {} vertices; the profile allows {limit}", geo.max_verts));
    }
    publish(&c, &bytes, &sections, &stats, json!({"bake": c.bake_ms, "lower": lower_ms, "crowd": crowd_ms, "textures": tex_ms, "total": c.t_all.elapsed().as_millis()}))?;
    println!("{}: {:.2} MB, {} meshes; props: near {} mid {} far {} backdrop {} triangles; {} large", c.output.display(), bytes.len() as f64 / 1e6, geo.recs.len(), geo.tris[0], geo.tris[1], geo.tris[2], geo.tris[3], geo.big);
    for (t, d) in &sections {
        println!("  {} {:>9}", String::from_utf8_lossy(&t.to_le_bytes()), d.len());
    }
    println!("models: {model_stats}");
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("requiem-cook: {e}");
        std::process::exit(1);
    }
}
