//! Pocket Requiem drawn with wgpu: the game of the handheld builds in a
//! browser tab over WebGPU, and on the build machine, where a frame can be
//! written to a file.
//!
//! The game is `requiem_sim`, the crate every device links. The pack is the
//! PS Vita's (`profiles/vita30.json`), read over HTTP ([`pack`]); the programs
//! are the PS Vita's, in WGSL ([`render`]). [`app`] is the shell around them:
//! the pad of each handheld the page shows, the frame, the readouts, the
//! sound. What is not this game's is PocketJS's browser kernel,
//! `pocket_web_wgpu` (`vendor/pocketjs/devices/web/pocket-web-wgpu`).

pub mod app;
pub mod hud;
pub mod mat;
pub mod pack;
pub mod render;
#[cfg(target_arch = "wasm32")]
mod web;
