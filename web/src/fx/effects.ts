// What each of the simulation's effects looks like: layers of FxIR, by the
// simulation's effect kind (`crates/requiem-sim/src/fx.rs`). The simulation
// gives an effect a place, a direction and one number (`a`: a reach, a
// radius, a side); everything else is here.
//
// Her magic is white with a blue edge. The undoing of the binding is gold,
// as her staff glows when she casts it; what leaves a freed knight is the
// pale flame the scales weigh.

import { FX, FX_LIFE } from "../sim/abi.gen";
import { SPRITE } from "./atlas";
import { Layer, lower, Lowered, Rgba } from "./ir";

const WHITE: Rgba = [1, 1, 1, 1];
const MANA: Rgba = [0.72, 0.88, 1, 0.9];
const DEEP: Rgba = [0.3, 0.52, 1, 0];
const GOLD: Rgba = [1, 0.88, 0.58, 1];
const GOLD_OUT: Rgba = [1, 0.72, 0.3, 0];
const SOUL: Rgba = [0.71, 0.94, 0.95, 0.9];
const SOUL_OUT: Rgba = [0.98, 0.9, 0.95, 0];
const FIRE: Rgba = [1, 0.72, 0.26, 0.95];
const FIRE_OUT: Rgba = [0.9, 0.14, 0.03, 0];
const VIOLET: Rgba = [0.86, 0.84, 1, 1];
const VIOLET_OUT: Rgba = [0.62, 0.5, 1, 0];
const STEEL: Rgba = [0.85, 0.9, 1, 0.7];
const out = (c: Rgba, a = 0): Rgba => [c[0], c[1], c[2], a];

/** The id the renderers give a bolt of the volley in flight: it is not one of the simulation's effect slots. */
export const BOLT_FX = FX_LIFE.length;

export const EFFECTS: Record<number, Layer[]> = {
  [FX.ARC]: [
    { type: "ring", plane: "swing", tilt: 0.1, signed: true, half: 1.4, inner: [0.9, 1.7], outer: [2.3, 3.3], height: 1.05, ease: 0.5, ends: true, sprite: SPRITE.ARC, color: [[0.72, 0.88, 1, 1], DEEP], fade: 0.9 },
    { type: "ring", plane: "swing", tilt: 0.1, signed: true, half: 1.4, inner: [2.15, 3.12], outer: [2.32, 3.32], height: 1.05, ease: 0.5, ends: true, sprite: SPRITE.SOFT, color: [WHITE, out(MANA)], fade: 0.9 },
    { type: "particles", count: 10, shape: "cone", angle: 1.3, speed: [3, 8], gravity: 6, life: [0.15, 0.3], size: [0.07, 0.02], sprite: SPRITE.STAR, color: [WHITE, out(MANA)], rise: 2.5, radius: 1.6 },
  ],
  [FX.SPARK]: [
    { type: "particles", count: 14, shape: "cone", angle: 1.1, speed: [3, 10], gravity: 16, drag: 0.4, life: [0.12, 0.3], size: [0.06, 0.02], stretch: 0.05, sprite: SPRITE.STREAK, color: [[1, 0.94, 0.75, 1], [1, 0.55, 0.2, 0]] },
    { type: "particles", count: 1, shape: "sphere", speed: [0, 0], life: [0.13, 0.13], size: [0.5, 1.0], sprite: SPRITE.DOT, color: [[0.85, 0.93, 1, 0.75], out(MANA)] },
    { type: "ring", plane: "facing", inner: [0.05, 0.6], outer: [0.16, 0.75], ease: 0.4, sprite: SPRITE.SOFT, color: [[1, 1, 1, 0.8], out(MANA)], segments: 20 },
  ],
  [FX.SHOCK]: [
    { type: "ring", plane: "ground", byA: true, inner: [0.1, 0.9], outer: [0.3, 1.0], height: 0.1, ease: 0.42, sprite: SPRITE.SOFT, color: [[0.85, 0.93, 1, 0.9], out(MANA)], fade: 1.2 },
    { type: "ring", plane: "ground", byA: true, inner: [0.0, 0.5], outer: [0.12, 0.6], height: 0.14, ease: 0.6, sprite: SPRITE.SOFT, color: [[1, 1, 1, 0.5], out(MANA)], fade: 1.6 },
    { type: "particles", count: 22, shape: "disc", speed: [2.5, 6.5], rise: 1.3, gravity: 5, drag: 0.6, life: [0.3, 0.55], size: [0.3, 0.8], vary: 0.4, sprite: SPRITE.SMOKE, blend: "over", color: [[0.42, 0.45, 0.55, 0.5], [0.3, 0.33, 0.42, 0]] },
  ],
  [FX.WHIRL]: [
    { type: "ring", plane: "ground", byA: true, half: 1.5, inner: [0.45, 0.6], outer: [0.92, 1.0], turn: [0.4, -9.5], height: 1.05, sprite: SPRITE.ARC, ends: true, color: [MANA, DEEP], fade: 0.8 },
    { type: "ring", plane: "ground", byA: true, half: 1.5, inner: [0.45, 0.6], outer: [0.92, 1.0], turn: [3.54, -9.5], height: 1.05, sprite: SPRITE.ARC, ends: true, color: [MANA, DEEP], fade: 0.8 },
    { type: "ring", plane: "ground", byA: true, inner: [0.86, 0.96], outer: [0.94, 1.02], height: 1.05, sprite: SPRITE.SOFT, color: [[1, 1, 1, 0.7], out(MANA)], fade: 1.0 },
    { type: "particles", count: 18, shape: "disc", speed: [4, 9], rise: 1.5, gravity: 4, life: [0.15, 0.35], size: [0.07, 0.02], sprite: SPRITE.STAR, color: [WHITE, out(MANA)], radius: 2.2 },
  ],
  [FX.CIRCLE]: [
    { type: "shell", mesh: "quad", plane: "facing", byA: true, radius: [0.25, 1.0], ease: 0.3, turn: [0, 2.2], ahead: 0.2, sprite: SPRITE.CIRCLE, color: [[0.85, 0.94, 1, 1], [0.6, 0.8, 1, 0]], fade: 0.5 },
    { type: "shell", mesh: "quad", plane: "facing", byA: true, radius: [0.5, 1.5], ease: 0.3, ahead: 0.2, sprite: SPRITE.DOT, color: [[0.5, 0.72, 1, 0.5], [0.4, 0.6, 1, 0]], fade: 0.8 },
    { type: "particles", count: 16, shape: "sphere", radius: 1.1, speed: [-2.4, -1.2], life: [0.3, 0.5], birth: 0.5, size: [0.03, 0.09], sprite: SPRITE.STAR, color: [out(MANA, 0.3), WHITE], fade: 0.4 },
  ],
  [FX.CONE]: [
    { type: "particles", count: 44, shape: "cone", angle: 0.5, byA: true, speed: [1.4, 2.8], drag: 0.7, life: [0.18, 0.4], size: [0.024, 0.006], stretch: 0.012, sprite: SPRITE.STREAK, color: [WHITE, out(MANA)] },
    { type: "ribbon", byA: true, length: [0.3, 0.95], width: [0.2, 0.02], grow: 0.3, sprite: SPRITE.BEAM, color: [[0.9, 0.96, 1, 0.9], DEEP], fade: 1.1 },
    { type: "ring", plane: "facing", byA: true, inner: [0.01, 0.2], outer: [0.05, 0.27], ease: 0.4, sprite: SPRITE.SOFT, color: [WHITE, out(MANA)] },
    { type: "particles", count: 1, shape: "sphere", byA: true, speed: [0, 0], life: [0.16, 0.16], size: [0.18, 0.42], sprite: SPRITE.DOT, color: [[0.9, 0.95, 1, 0.9], out(MANA)] },
  ],
  [FX.BURST]: [
    { type: "particles", count: 28, shape: "sphere", byA: true, speed: [0.9, 2.6], drag: 0.8, gravity: 3, life: [0.2, 0.45], size: [0.036, 0.01], stretch: 0.03, sprite: SPRITE.STREAK, color: [WHITE, out(MANA)] },
    { type: "particles", count: 1, shape: "sphere", byA: true, speed: [0, 0], life: [0.2, 0.2], size: [0.5, 1.1], sprite: SPRITE.DOT, color: [[0.9, 0.95, 1, 0.95], out(MANA)] },
    { type: "ring", plane: "ground", byA: true, inner: [0.1, 0.9], outer: [0.3, 1.0], ease: 0.4, height: -0.2, sprite: SPRITE.SOFT, color: [[0.85, 0.93, 1, 0.8], out(MANA)], fade: 1.3 },
  ],
  [FX.BEAM]: [
    { type: "ribbon", byA: true, length: [1, 1], width: [0.022, 0.0], grow: 0.07, sprite: SPRITE.BEAM, color: [WHITE, [0.8, 0.9, 1, 0]], fade: 0.6 },
    { type: "ribbon", byA: true, length: [1, 1], width: [0.05, 0.012], grow: 0.07, sprite: SPRITE.SOFT, color: [[0.42, 0.68, 1, 0.5], DEEP], fade: 0.9 },
    { type: "particles", count: 48, shape: "cone", angle: 0.04, byA: true, speed: [0.5, 1.1], life: [0.3, 0.7], birth: 0.25, size: [0.007, 0.002], sprite: SPRITE.STAR, color: [WHITE, out(MANA)] },
    { type: "ring", plane: "facing", byA: true, inner: [0.0, 0.034], outer: [0.008, 0.046], ease: 0.35, sprite: SPRITE.SOFT, color: [WHITE, out(MANA)] },
    { type: "ring", plane: "facing", byA: true, inner: [0.0, 0.016], outer: [0.006, 0.024], ease: 0.6, height: 0, sprite: SPRITE.SOFT, color: [[0.8, 0.9, 1, 0.8], out(MANA)] },
  ],
  [FX.PILLAR]: [
    { type: "shell", mesh: "cylinder", plane: "ground", byA: true, radius: [0.45, 1.0], height: [0.3, 2.4], ease: 0.35, sprite: SPRITE.SOFT, color: [[0.72, 0.88, 1, 0.8], DEEP], rim: [1.5, 0.8], base: 0.4, fade: 1.2 },
    { type: "shell", mesh: "cylinder", plane: "ground", byA: true, radius: [0.2, 0.55], height: [0.5, 3.2], ease: 0.35, sprite: SPRITE.SOFT, color: [[1, 1, 1, 0.7], out(MANA)], base: 0.7, fade: 1.6 },
    { type: "particles", count: 30, shape: "disc", byA: true, radius: 0.5, speed: [0.05, 0.3], rise: 5, life: [0.35, 0.8], birth: 0.4, size: [0.03, 0.008], sprite: SPRITE.STAR, color: [WHITE, out(MANA)] },
    { type: "ring", plane: "ground", byA: true, inner: [0.3, 1.0], outer: [0.5, 1.15], ease: 0.4, height: 0.1, sprite: SPRITE.SOFT, color: [[0.85, 0.93, 1, 0.8], out(MANA)] },
  ],
  [FX.LIGHTNING]: [
    { type: "ribbon", byA: true, fan: 5, spread: 0.62, length: [1, 1], width: [0.01, 0.004], grow: 0.05, jag: 0.055, flicker: 30, segments: 14, sprite: SPRITE.BOLT, color: [VIOLET, VIOLET_OUT], fade: 0.5 },
    { type: "ribbon", byA: true, fan: 5, spread: 0.62, length: [1, 1], width: [0.028, 0.012], grow: 0.05, jag: 0.055, flicker: 30, segments: 14, sprite: SPRITE.SOFT, color: [[0.6, 0.55, 1, 0.22], VIOLET_OUT], fade: 0.7 },
    { type: "particles", count: 1, shape: "sphere", byA: true, speed: [0, 0], life: [0.1, 0.1], size: [0.07, 0.14], sprite: SPRITE.DOT, color: [[0.86, 0.84, 1, 0.7], VIOLET_OUT] },
  ],
  [FX.JOLT]: [
    { type: "ribbon", upright: true, length: [2.4, 2.4], width: [0.12, 0.03], grow: 0.05, jag: 0.12, flicker: 30, lift: -1.1, segments: 8, sprite: SPRITE.BOLT, color: [VIOLET, VIOLET_OUT], fade: 0.6 },
    { type: "particles", count: 9, shape: "sphere", speed: [2, 6], gravity: 12, life: [0.12, 0.3], size: [0.06, 0.02], stretch: 0.05, sprite: SPRITE.STREAK, color: [VIOLET, VIOLET_OUT] },
  ],
  [FX.HELLFIRE]: [
    { type: "shell", mesh: "dome", plane: "ground", byA: true, radius: [0.12, 1.0], height: [0.12, 0.75], ease: 0.28, sprite: SPRITE.SOFT, color: [[1, 0.62, 0.22, 0.5], FIRE_OUT], rim: [1.4, 0.7], base: 0.3, fade: 2.6 },
    { type: "particles", count: 46, shape: "cone", angle: 1.5, byA: true, radius: 0.2, speed: [0.1, 0.5], rise: 2.6, life: [0.5, 1.0], birth: 0.35, size: [0.2, 0.42], vary: 0.4, sprite: SPRITE.FLAME, color: [FIRE, FIRE_OUT], fade: 0.8 },
    { type: "particles", count: 28, shape: "cone", angle: 1.5, byA: true, radius: 0.3, speed: [0.05, 0.3], rise: 2.2, life: [0.8, 1.3], birth: 0.4, size: [0.2, 0.5], vary: 0.3, sprite: SPRITE.SMOKE, blend: "over", color: [[0.1, 0.08, 0.08, 0.55], [0.04, 0.04, 0.05, 0]] },
    { type: "particles", count: 44, shape: "cone", angle: 1.2, byA: true, speed: [0.3, 1.1], rise: 3.5, gravity: 4, drag: 0.5, life: [0.6, 1.25], birth: 0.2, size: [0.013, 0.004], sprite: SPRITE.STAR, color: [[1, 0.8, 0.4, 1], [1, 0.3, 0.05, 0]] },
    { type: "ring", plane: "ground", byA: true, inner: [0.1, 1.1], outer: [0.32, 1.28], ease: 0.33, height: 0.12, sprite: SPRITE.SOFT, color: [[1, 0.7, 0.3, 0.6], FIRE_OUT], fade: 1.6 },
    { type: "particles", count: 1, shape: "sphere", byA: true, speed: [0, 0], life: [0.3, 0.3], size: [0.3, 0.75], sprite: SPRITE.DOT, color: [[1, 0.82, 0.5, 0.7], FIRE_OUT] },
  ],
  [FX.EMBER]: [{ type: "particles", count: 8, shape: "cone", angle: 0.9, speed: [0.3, 1.2], rise: 1.6, life: [0.25, 0.5], birth: 0.3, size: [0.35, 0.6], vary: 0.4, sprite: SPRITE.FLAME, color: [FIRE, FIRE_OUT] }],
  [FX.BLINK]: [
    { type: "ribbon", length: [0.5, 3.4], width: [0.7, 0.0], grow: 1, lift: 1.0, sprite: SPRITE.SOFT, color: [[0.7, 0.85, 1, 0.5], out(MANA)], fade: 1.2 },
    { type: "particles", count: 18, shape: "sphere", speed: [0.3, 1.6], rise: 0.6, life: [0.2, 0.45], birth: 0.6, size: [0.09, 0.02], sprite: SPRITE.STAR, color: [WHITE, out(MANA)], radius: 0.5 },
  ],
  [FX.GATHER]: [
    { type: "particles", count: 64, shape: "sphere", byA: true, radius: 0.34, speed: [-0.5, -0.24], life: [0.5, 0.95], birth: 0.82, size: [0.004, 0.012], sprite: SPRITE.STAR, color: [out(GOLD, 0.25), GOLD], fade: 0.3 },
    { type: "shell", mesh: "quad", plane: "ground", byA: true, radius: [0.03, 0.2], turn: [0, 3.2], lift: 0.08, ease: 0.5, sprite: SPRITE.CIRCLE, color: [out(GOLD, 0.5), GOLD], fade: 0.2 },
    { type: "ring", plane: "ground", byA: true, inner: [0.5, 0.02], outer: [0.56, 0.07], height: 0.1, sprite: SPRITE.SOFT, color: [out(GOLD, 0.2), GOLD], fade: 0.3 },
    { type: "shell", mesh: "cylinder", plane: "ground", byA: true, radius: [0.05, 0.035], height: [0.1, 0.5], sprite: SPRITE.SOFT, color: [out(GOLD, 0.15), [1, 0.92, 0.7, 0.7]], base: 0.6, fade: 0.25 },
  ],
  [FX.UNSEAL]: [
    { type: "shell", mesh: "dome", plane: "ground", byA: true, radius: [0.04, 1.0], height: [0.04, 0.6], ease: 0.42, sprite: SPRITE.SOFT, color: [[1, 0.95, 0.8, 0.7], [0.7, 0.85, 1, 0]], rim: [2.4, 1.0], base: 0.08, fade: 1.3 },
    { type: "ring", plane: "ground", byA: true, inner: [0.02, 0.95], outer: [0.1, 1.03], ease: 0.42, height: 0.15, sprite: SPRITE.SOFT, color: [WHITE, [0.8, 0.9, 1, 0]], fade: 0.9 },
    { type: "ring", plane: "ground", byA: true, inner: [0.0, 0.7], outer: [0.05, 0.8], ease: 0.6, height: 0.12, sprite: SPRITE.SOFT, color: [GOLD, GOLD_OUT], fade: 1.2 },
    { type: "particles", count: 72, shape: "disc", byA: true, speed: [0.25, 1.0], rise: 4.2, life: [0.6, 1.15], birth: 0.3, size: [0.006, 0.002], sprite: SPRITE.STAR, color: [GOLD, GOLD_OUT] },
    { type: "particles", count: 1, shape: "sphere", byA: true, speed: [0, 0], life: [0.28, 0.28], size: [0.25, 0.8], sprite: SPRITE.DOT, color: [[1, 0.96, 0.85, 1], GOLD_OUT] },
    { type: "shell", mesh: "quad", plane: "ground", byA: true, radius: [0.2, 0.34], turn: [0, 1.4], lift: 0.1, sprite: SPRITE.CIRCLE, color: [GOLD, GOLD_OUT], fade: 1.2 },
  ],
  [FX.SOUL]: [
    { type: "particles", count: 9, shape: "sphere", speed: [0.05, 0.35], rise: 0.95, life: [0.9, 1.7], birth: 0.4, size: [0.08, 0.02], sprite: SPRITE.STAR, color: [SOUL, SOUL_OUT], fade: 0.7 },
    { type: "ribbon", upright: true, length: [0.3, 2.6], width: [0.2, 0.02], grow: 1, lift: 0.1, sprite: SPRITE.SOFT, color: [out(SOUL, 0.5), SOUL_OUT], fade: 1.4 },
    { type: "particles", count: 1, shape: "sphere", speed: [0, 0], rise: 0.5, life: [1.2, 1.2], size: [0.5, 0.2], sprite: SPRITE.DOT, color: [out(SOUL, 0.5), SOUL_OUT] },
  ],
  [FX.DUST]: [{ type: "particles", count: 12, shape: "disc", speed: [0.8, 2.4], rise: 0.7, gravity: 1.2, drag: 0.7, life: [0.35, 0.66], size: [0.3, 0.95], vary: 0.4, sprite: SPRITE.SMOKE, blend: "over", color: [[0.34, 0.36, 0.44, 0.45], [0.28, 0.3, 0.38, 0]] }],
  [FX.GUARD]: [
    { type: "shell", mesh: "panel", plane: "ground", radius: [1.05, 1.05], height: [1.9, 1.9], lift: 0.1, sprite: SPRITE.HEX, color: [[0.5, 0.76, 1, 0.7], [0.5, 0.76, 1, 0.35]], rim: [2, 0.5], base: 0.65, fade: 0.4 },
  ],
  [FX.BLOCK]: [
    { type: "shell", mesh: "quad", plane: "facing", radius: [0.5, 1.2], ease: 0.4, sprite: SPRITE.HEX, color: [[0.8, 0.92, 1, 1], out(MANA)], fade: 1 },
    { type: "ring", plane: "facing", inner: [0.1, 0.8], outer: [0.24, 0.95], ease: 0.4, sprite: SPRITE.SOFT, color: [WHITE, out(MANA)] },
    { type: "particles", count: 10, shape: "sphere", speed: [2, 6], gravity: 10, life: [0.12, 0.28], size: [0.05, 0.02], stretch: 0.05, sprite: SPRITE.STREAK, color: [WHITE, out(MANA)] },
  ],
  [FX.SLASH]: [{ type: "ring", plane: "swing", tilt: 0.6, half: 1.0, byA: true, inner: [0.45, 0.55], outer: [0.94, 1.02], ease: 0.5, ends: true, sprite: SPRITE.ARC, color: [STEEL, [0.6, 0.7, 0.9, 0]], fade: 1.2 }],
  [FX.HURT]: [
    { type: "particles", count: 10, shape: "sphere", speed: [2, 5.5], gravity: 7, life: [0.15, 0.3], size: [0.07, 0.02], stretch: 0.05, sprite: SPRITE.STREAK, color: [[1, 0.7, 0.7, 1], [1, 0.2, 0.2, 0]] },
    { type: "particles", count: 1, shape: "sphere", speed: [0, 0], life: [0.14, 0.14], size: [0.6, 1.1], sprite: SPRITE.DOT, color: [[1, 0.5, 0.5, 0.7], [1, 0.2, 0.2, 0]] },
  ],
  [FX.MUZZLE]: [
    { type: "shell", mesh: "quad", plane: "facing", radius: [0.3, 0.7], ease: 0.4, turn: [0, 1.5], sprite: SPRITE.CIRCLE, color: [[0.85, 0.94, 1, 1], out(MANA)], fade: 0.8 },
    { type: "particles", count: 1, shape: "sphere", speed: [0, 0], life: [0.1, 0.1], size: [0.4, 0.8], sprite: SPRITE.DOT, color: [[0.9, 0.95, 1, 0.9], out(MANA)] },
  ],
  // A bolt in flight: the renderers hold it at one age and point it back along its path.
  [BOLT_FX]: [
    { type: "ribbon", length: [2.6, 2.6], width: [0.3, 0.3], grow: 0.01, sprite: SPRITE.BEAM, color: [WHITE, WHITE], fade: 0 },
    { type: "ribbon", length: [3.4, 3.4], width: [0.7, 0.7], grow: 0.01, sprite: SPRITE.SOFT, color: [[0.4, 0.66, 1, 0.5], [0.4, 0.66, 1, 0.5]], fade: 0 },
    { type: "particles", count: 1, shape: "sphere", speed: [0, 0], life: [9, 9], size: [0.5, 0.5], sprite: SPRITE.DOT, color: [WHITE, WHITE], fade: 0, appear: 0.001 },
  ],
};

export interface CompiledFx {
  /** Per effect id (the simulation's kinds, then `BOLT_FX`): its lowered layers. */
  effects: Lowered[][];
}

export function compileEffects(): CompiledFx {
  const effects: Lowered[][] = [];
  for (let kind = 0; kind <= BOLT_FX; kind++) {
    const life = (kind < FX_LIFE.length ? FX_LIFE[kind] : 60) / 60;
    effects.push((EFFECTS[kind] ?? []).map((layer, k) => lower(layer, life, kind * 16 + k + 1)));
  }
  return { effects };
}
