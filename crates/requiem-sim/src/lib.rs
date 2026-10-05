//! Deterministic simulation core of Pocket Requiem.
//!
//! The same crate runs in the three.js reference (as wasm), in the compiler
//! (which samples the knights' clips to bake their frames, and casts rays
//! for the light bake) and on every device. `abi` is the C interface the
//! wasm host uses; native hosts use `Sim` directly.

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(feature = "hw-sqrt", feature(core_intrinsics), allow(internal_features))]

extern crate alloc;

pub mod abi;
pub mod anim;
pub mod audio;
pub mod auto;
pub mod collide;
pub mod crowd;
pub mod demon;
pub mod fastmath;
pub mod field;
pub mod mage;
pub mod fx;
pub mod knight;
pub mod math;
pub mod moves;
pub mod sim;
pub mod skel;
pub mod worldfile;

pub use sim::{Input, Sim};
