// Light, air and level-of-detail distances of the night field. The reference
// renderer lights with these at run time; the compiler bakes the same model
// into vertex colours, so both read one table.
//
// Colours are linear RGB. Radiance leaving a surface is
// `albedo × (moon × max(N·L, 0) × visibility + hemisphere(N) × occlusion)`,
// plus what the spells cast at play time.

export const SCENE = {
  /** Unit vector toward the moon: low in the north, ahead of the mage and a little to her left, over the army. */
  sunDir: [-0.3035, 0.4067, -0.8617] as const,
  sun: [0.5, 0.66, 1.0] as const,
  /** Ambient from straight up and from straight down. */
  sky: [0.085, 0.15, 0.4] as const,
  bounce: [0.02, 0.036, 0.085] as const,
  /** Haze: colour and the density of `1 - exp(-(d × density)²)`. */
  fog: [0.016, 0.06, 0.25] as const,
  fogDensity: 0.0021,
  /** Sky gradient: at the horizon, at the zenith, and the glow around the moon. */
  horizon: [0.02, 0.075, 0.3] as const,
  zenith: [0.0035, 0.024, 0.18] as const,
  glow: [0.3, 0.5, 0.9] as const,
  /** The moon's disc: its colour and its angular radius in radians (it is drawn large). */
  moon: [0.62, 0.78, 0.92] as const,
  moonRadius: 0.105,
  /** A near cell draws its detailed mesh inside `near` metres and its simple one beyond; super-cells past `mid` draw the far mesh. */
  lod: { near: 150, mid: 520 },
  clip: { near: 0.35, far: 6000 },
} as const;
