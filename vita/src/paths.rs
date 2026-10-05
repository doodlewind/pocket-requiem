//! Where files live on the console and on the development computer.

/// The USB share's folder for this app, when the wired debug host runs.
pub const HOST: &str = "host0:requiem";
pub const GXP_HOST: &str = "host0:requiem/gxp";
/// This app's folder on the memory card.
pub const DATA: &str = "ux0:data/pocket-requiem";
/// Programs compiled on this console.
pub const GXP_CACHE: &str = "ux0:data/pocket-requiem/gxp";
/// Programs shipped in the package.
pub const GXP_PACKAGED: &str = "app0:gxp";
/// The stage pack: the development copy first, then the packaged one.
pub const PACKS: [&str; 3] = ["host0:requiem/stage.pack", "app0:stage.pack", "ux0:data/pocket-requiem/stage.pack"];
