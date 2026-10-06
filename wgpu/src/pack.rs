//! The pack over HTTP: what a first frame needs, then the rest behind the
//! fight.
//!
//! The PS Vita reads its pack whole before it draws. A tab on a slow line
//! would wait half a minute for that, so the reads are in two sets. **The
//! first set is everything but the two largest parts a frame can do without
//! for a while**: the detailed meshes of the field's cells (a cell draws its
//! simple mesh until its detailed one is here) and the stored frames of the
//! army's two finest levels of detail (a knight is drawn a level coarser
//! until they are). The fight starts when the first set has arrived; the
//! second set follows, and the shell hands each read to the renderer as it
//! arrives.
//!
//! Every read is a range of [`Source`] of one size, four side by side, so a
//! pack cut into pieces of that size is fetched a piece a request.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pocket_web_wgpu::source::Source;
use pocket_web_wgpu::task;
use requiem_pack::{self as pack, mesh_kind, CrowdHeader, CrowdMesh, CrowdVertex, MeshRec};

/// Bytes of one read: the size `tools/wgpu.ts dist` cuts the pack's pieces to.
pub const READ: u64 = 1 << 20;
/// Reads in flight at once.
const SIDE_BY_SIDE: usize = 4;

/// Ranges of bytes of a pack, in order and apart from each other.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ranges(Vec<(u64, u64)>);

impl Ranges {
    pub fn add(&mut self, from: u64, to: u64) {
        if from >= to {
            return;
        }
        self.0.push((from, to));
        self.0.sort_unstable();
        let mut merged: Vec<(u64, u64)> = Vec::with_capacity(self.0.len());
        for &(a, b) in &self.0 {
            match merged.last_mut() {
                Some(last) if a <= last.1 => last.1 = last.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        self.0 = merged;
    }

    /// Whether every byte from `from` to `to` is in one of the ranges.
    pub fn cover(&self, from: u64, to: u64) -> bool {
        from >= to || self.0.iter().any(|&(a, b)| a <= from && to <= b)
    }

    pub fn bytes(&self) -> u64 {
        self.0.iter().map(|&(a, b)| b - a).sum()
    }
}

/// A pack's section table: tag, offset, size.
fn table(head: &[u8]) -> Vec<(u32, u64, u64)> {
    let word = |at: usize| head.get(at..at + 4).map_or(0, |n| u32::from_le_bytes([n[0], n[1], n[2], n[3]]));
    (0..(word(8) as usize).min(64)).map(|i| 16 + i * 16).map(|at| (word(at), word(at + 4) as u64, word(at + 8) as u64)).collect()
}

/// What a first frame does without: the vertices and indices of the field's detailed meshes, and the stored
/// frames of the army's two finest levels. `meshes` is the pack's `MESH` section, `crowd` the head of its
/// `CRWD` section with its mesh records; the offsets are those of `VTX0`, `IDX0` and `CRWD` in the pack.
pub fn later(meshes: &[u8], crowd: &[u8], vtx_at: u64, idx_at: u64, crowd_at: u64) -> Ranges {
    let mut later = Ranges::default();
    let size = core::mem::size_of::<MeshRec>();
    for r in (0..meshes.len() / size).filter_map(|i| pack::read::<MeshRec>(meshes, i * size)).filter(|r| r.kind == mesh_kind::NEAR) {
        later.add(vtx_at + r.vtx_first as u64 * 16, vtx_at + (r.vtx_first + r.vtx_count) as u64 * 16);
        later.add(idx_at + r.idx_first as u64 * 2, idx_at + (r.idx_first + r.idx_count) as u64 * 2);
    }
    if let Some(head) = pack::read::<CrowdHeader>(crowd, 0) {
        for m in (0..(head.kinds * head.lods) as usize).filter_map(|i| pack::read::<CrowdMesh>(crowd, core::mem::size_of::<CrowdHeader>() + i * core::mem::size_of::<CrowdMesh>())).filter(|m| m.lod < 2 && head.lods > 2) {
            let frames = head.frames as u64 * m.vtx_count as u64 * core::mem::size_of::<CrowdVertex>() as u64;
            later.add(crowd_at + m.frames_at as u64, crowd_at + m.frames_at as u64 + frames);
        }
    }
    later
}

/// A pack on its way.
#[derive(Clone, Default)]
pub struct Coming {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    total: Cell<u64>,
    /// Bytes of the first set, and how many of them have arrived.
    first: Cell<u64>,
    arrived: Cell<u64>,
    /// Bytes of both sets that have arrived.
    all: Cell<u64>,
    bytes: RefCell<Vec<u8>>,
    /// The `FONT` section, read before the rest: the shell says what is read with it.
    font: RefCell<Option<Vec<u8>>>,
    /// Offsets not yet asked for, the next one last: the first set's, then the second's.
    left: RefCell<Vec<(u64, bool)>>,
    /// Reads of the first set not yet arrived.
    waiting: Cell<usize>,
    /// What has arrived and is in `bytes`.
    have: RefCell<Ranges>,
    ready: Cell<bool>,
    taken: Cell<bool>,
    /// Reads that arrived after the pack was taken, with their offsets.
    late: RefCell<Vec<(u64, Vec<u8>)>>,
    failed: RefCell<Option<String>>,
}

impl Coming {
    /// Starts reading the pack at `place`: its file on a server that answers byte ranges, or the manifest
    /// (`.json`) of a pack cut into pieces. Outside a tab every read has happened when this returns.
    pub fn start(place: String) -> Coming {
        let coming = Coming::default();
        let inner = coming.inner.clone();
        task::spawn(async move {
            let opened = async {
                let source = Source::open(&place).await?;
                let total = source.length().await?;
                // The table says where everything is. The glyphs are read first, then the two tables that
                // say which bytes can wait.
                let sections = table(&source.range(0, total.min(16 + 64 * 16)).await?);
                let of = |tag: u32| sections.iter().find(|s| s.0 == tag).map(|s| (s.1, s.2)).ok_or_else(|| format!("{place}: not a pack of Pocket Requiem"));
                let font = of(pack::FONT)?;
                *inner.font.borrow_mut() = Some(source.range(font.0, font.1).await?);
                let (meshes, crowd) = (of(pack::MESH)?, of(pack::CRWD)?);
                let later = later(&source.range(meshes.0, meshes.1).await?, &source.range(crowd.0, crowd.1.min(4096)).await?, of(pack::VTX0)?.0, of(pack::IDX0)?.0, crowd.0);
                Ok::<_, String>((source, total, later))
            };
            let (source, total, later) = match opened.await {
                Ok(opened) => opened,
                Err(e) => return inner.fail(e),
            };
            *inner.bytes.borrow_mut() = vec![0; total as usize];
            // A read is of the second set when all of it can wait.
            let reads: Vec<(u64, bool)> = (0..total.div_ceil(READ)).map(|i| i * READ).map(|at| (at, later.cover(at, (at + READ).min(total)))).collect();
            let first: Vec<&(u64, bool)> = reads.iter().filter(|r| !r.1).collect();
            inner.first.set(first.iter().map(|r| READ.min(total - r.0)).sum());
            inner.waiting.set(first.len());
            *inner.left.borrow_mut() = reads.iter().filter(|r| r.1).rev().chain(first.into_iter().rev()).copied().collect();
            inner.total.set(total);
            for _ in 0..SIDE_BY_SIDE {
                let (inner, source) = (inner.clone(), source.clone());
                task::spawn(async move {
                    loop {
                        let Some((offset, second)) = inner.left.borrow_mut().pop() else { break };
                        let size = READ.min(inner.total.get() - offset);
                        let part = match source.range(offset, size).await {
                            Ok(part) => part,
                            Err(e) => return inner.fail(e),
                        };
                        inner.all.set(inner.all.get() + size);
                        if inner.taken.get() {
                            inner.late.borrow_mut().push((offset, part));
                            continue;
                        }
                        inner.bytes.borrow_mut()[offset as usize..(offset + size) as usize].copy_from_slice(&part);
                        inner.have.borrow_mut().add(offset, offset + size);
                        if !second {
                            inner.arrived.set(inner.arrived.get() + size);
                            inner.waiting.set(inner.waiting.get() - 1);
                            inner.ready.set(inner.waiting.get() == 0);
                        }
                    }
                });
            }
        });
        coming
    }

    /// Bytes of the first set arrived, and bytes of the first set (0 until the pack's tables are known).
    pub fn progress(&self) -> (u64, u64) {
        (self.inner.arrived.get(), self.inner.first.get())
    }

    /// Bytes of the whole pack arrived, and its size.
    pub fn streamed(&self) -> (u64, u64) {
        (self.inner.all.get(), self.inner.total.get())
    }

    /// The pack's `FONT` section, once, when it has arrived.
    pub fn font(&self) -> Option<Vec<u8>> {
        self.inner.font.borrow_mut().take()
    }

    pub fn failure(&self) -> Option<String> {
        self.inner.failed.borrow().clone()
    }

    /// The pack, once, when the first set has arrived: its bytes, with nothing where a read of the second
    /// set has not arrived yet, and which bytes are there.
    pub fn take(&self) -> Option<(Vec<u8>, Ranges)> {
        if !self.inner.ready.get() || self.inner.taken.replace(true) {
            return None;
        }
        Some((std::mem::take(&mut *self.inner.bytes.borrow_mut()), self.inner.have.borrow().clone()))
    }

    /// A read that arrived after the pack was taken: its offset in the pack and its bytes.
    pub fn late(&self) -> Option<(u64, Vec<u8>)> {
        self.inner.late.borrow_mut().pop()
    }
}

impl Inner {
    fn fail(&self, why: String) {
        self.left.borrow_mut().clear();
        self.failed.borrow_mut().get_or_insert(why);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_join_and_cover() {
        let mut r = Ranges::default();
        r.add(10, 20);
        r.add(30, 40);
        assert!(r.cover(12, 20) && !r.cover(12, 21) && !r.cover(20, 30) && r.cover(5, 5));
        r.add(20, 30);
        assert_eq!(r, Ranges(vec![(10, 40)]));
        assert!(r.cover(10, 40) && r.bytes() == 30);
    }

    #[test]
    fn the_detailed_meshes_and_the_two_finest_levels_wait() {
        let near = MeshRec { kind: mesh_kind::NEAR, vtx_first: 0, vtx_count: 100, idx_first: 0, idx_count: 300, ..Default::default() };
        let mid = MeshRec { kind: mesh_kind::MID, vtx_first: 100, vtx_count: 50, idx_first: 300, idx_count: 90, ..Default::default() };
        let meshes = [pack::bytes_of(&near), pack::bytes_of(&mid)].concat();
        let head = CrowdHeader { kinds: 1, lods: 3, frames: 10, scale: 4.0 };
        let level = |lod: u32, frames_at: u32| CrowdMesh { kind: 0, lod, vtx_count: 8, idx_count: 12, color_at: 0, idx_at: 0, frames_at, pad: 0 };
        let crowd = [pack::bytes_of(&head), pack::bytes_of(&level(0, 1000)), pack::bytes_of(&level(1, 2000)), pack::bytes_of(&level(2, 3000))].concat();
        let later = later(&meshes, &crowd, 10_000, 20_000, 30_000);
        // The detailed mesh's vertices and indices, and ten frames of eight vertices of twelve bytes for each of two levels.
        assert_eq!(later, Ranges(vec![(10_000, 11_600), (20_000, 20_600), (31_000, 31_960), (32_000, 32_960)]));
        // A pack whose army has two levels keeps both: there is no coarser one to draw in their place.
        let two = [pack::bytes_of(&CrowdHeader { lods: 2, ..head }), pack::bytes_of(&level(0, 1000)), pack::bytes_of(&level(1, 2000))].concat();
        assert_eq!(super::later(&[], &two, 0, 0, 0), Ranges::default());
    }
}
