//! The pack over HTTP: the section table first, then every section, a few
//! reads at a time.
//!
//! The PS Vita reads its pack whole before it draws, and so does a tab: the
//! field's meshes and the army's stored frames are most of the file and a
//! first frame needs both. The reads are ranges of [`Source`] of one size, so
//! a pack cut into pieces is fetched a piece a request, four side by side,
//! and the shell can say how much has arrived.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pocket_web_wgpu::source::Source;
use pocket_web_wgpu::task;

/// Bytes of one read.
const READ: u64 = 2 << 20;
/// Reads in flight at once.
const SIDE_BY_SIDE: usize = 4;

/// A pack on its way: how much has arrived, and the whole when all of it has.
#[derive(Clone, Default)]
pub struct Coming {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    total: Cell<u64>,
    arrived: Cell<u64>,
    bytes: RefCell<Vec<u8>>,
    /// The `FONT` section, read before the rest: the shell says what is read with it.
    font: RefCell<Option<Vec<u8>>>,
    /// Offsets not yet asked for, the last first.
    left: RefCell<Vec<u64>>,
    done: Cell<bool>,
    failed: RefCell<Option<String>>,
}

impl Coming {
    /// Starts reading the pack at `place`: its file on a server that answers byte ranges, or the manifest
    /// (`.json`) of a pack cut into pieces. Outside a tab the whole read has happened when this returns.
    pub fn start(place: String) -> Coming {
        let coming = Coming::default();
        let inner = coming.inner.clone();
        task::spawn(async move {
            let opened = async {
                let source = Source::open(&place).await?;
                let total = source.length().await?;
                Ok::<_, String>((source, total))
            };
            let (source, total) = match opened.await {
                Ok(opened) => opened,
                Err(e) => return inner.fail(e),
            };
            // The table says where the glyphs are; they are read first.
            let font = async {
                let head = source.range(0, total.min(16 + 64 * 16)).await?;
                let count = head.get(8..12).map_or(0, |n| u32::from_le_bytes([n[0], n[1], n[2], n[3]]) as usize).min(64);
                let word = |at: usize| head.get(at..at + 4).map_or(0, |n| u32::from_le_bytes([n[0], n[1], n[2], n[3]]));
                match (0..count).map(|i| 16 + i * 16).find(|&at| word(at) == requiem_pack::FONT) {
                    Some(at) => source.range(word(at + 4) as u64, word(at + 8) as u64).await,
                    None => Err(format!("{place}: not a pack of Pocket Requiem")),
                }
            };
            match font.await {
                Ok(font) => *inner.font.borrow_mut() = Some(font),
                Err(e) => return inner.fail(e),
            }
            inner.total.set(total);
            *inner.bytes.borrow_mut() = vec![0; total as usize];
            *inner.left.borrow_mut() = (0..total.div_ceil(READ)).rev().map(|i| i * READ).collect();
            for _ in 0..SIDE_BY_SIDE {
                let (inner, source) = (inner.clone(), source.clone());
                task::spawn(async move {
                    loop {
                        let Some(offset) = inner.left.borrow_mut().pop() else { break };
                        let size = READ.min(inner.total.get() - offset);
                        match source.range(offset, size).await {
                            Ok(part) => {
                                inner.bytes.borrow_mut()[offset as usize..(offset + size) as usize].copy_from_slice(&part);
                                inner.arrived.set(inner.arrived.get() + size);
                            }
                            Err(e) => return inner.fail(e),
                        }
                    }
                    if inner.arrived.get() == inner.total.get() {
                        inner.done.set(true);
                    }
                });
            }
        });
        coming
    }

    /// Bytes arrived and bytes in all (0 until the pack's length is known).
    pub fn progress(&self) -> (u64, u64) {
        (self.inner.arrived.get(), self.inner.total.get())
    }

    /// The pack's `FONT` section, once, when it has arrived.
    pub fn font(&self) -> Option<Vec<u8>> {
        self.inner.font.borrow_mut().take()
    }

    pub fn failure(&self) -> Option<String> {
        self.inner.failed.borrow().clone()
    }

    /// The whole pack, once, when every read has arrived.
    pub fn take(&self) -> Option<Vec<u8>> {
        if !self.inner.done.replace(false) {
            return None;
        }
        Some(std::mem::take(&mut *self.inner.bytes.borrow_mut()))
    }
}

impl Inner {
    fn fail(&self, why: String) {
        self.left.borrow_mut().clear();
        self.failed.borrow_mut().get_or_insert(why);
    }
}
