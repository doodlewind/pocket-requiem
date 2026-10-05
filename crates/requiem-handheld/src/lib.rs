//! The device-independent half of the handheld runtimes (PSP, 3DS).
//!
//! A device crate owns the GPU, the pad, the sound output and storage. This
//! crate owns everything those share: which prop meshes a frame draws
//! (`world`), the ground built from its two grids (`ground`), which knights a
//! frame draws and at what level of detail (`crowd`), the effects evaluated
//! into vertices (`fx`), the sky and the shadows (`figures`), the interface
//! (`hud`), the loop around the simulation (`game`), and the PSP's guard-band
//! clipping (`clip`), kept here so a computer can test it.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod clip;
pub mod crowd;
pub mod figures;
pub mod fx;
pub mod game;
pub mod ground;
pub mod hud;
pub mod mat;
pub mod scene;
pub mod world;
