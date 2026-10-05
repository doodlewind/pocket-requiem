// The simulation core (crates/requiem-sim) as wasm. The reference never
// re-implements a rule: it feeds inputs and reads the snapshot, the crowd's
// draw list, the effect slots and the bolts.

import { ABI_VERSION, BOLT_BYTES, BOLTS, BONES, DRAW_BYTES, FX_BYTES, FX_SLOTS, SNAP_LEN } from "./abi.gen";

interface Exports {
  memory: WebAssembly.Memory;
  rq_abi_version(): number;
  rq_alloc(len: number): number;
  rq_free(ptr: number, len: number): void;
  rq_load(ptr: number, len: number): number;
  rq_load_test(cohorts: number): void;
  rq_reset(): void;
  rq_tick(buttons: number, lx: number, ly: number, rx: number, ry: number): void;
  rq_tick_auto(): number;
  rq_snapshot(): number;
  rq_snapshot_len(): number;
  rq_planes(): number;
  rq_crowd(ex: number, ey: number, ez: number, far: number): number;
  rq_crowd_ptr(): number;
  rq_fx(): number;
  rq_bolts(): number;
  rq_height(x: number, z: number): number;
  rq_audio(frames: number, rate: number): number;
  rq_bind(kind: number): number;
  rq_knight_frame(kind: number, frame: number): number;
  rq_mage_pose(show: number, mv: number, t: number, phase: number, speed: number, hover: number): number;
  rq_demon(): number;
}

/** One knight to draw: fields of `crowd::Draw`. */
export interface CrowdView {
  count: number;
  /** Float view over the records (`DRAW_BYTES / 4` floats each). */
  f: Float32Array;
  /** Byte view over the same records. */
  u16: Uint16Array;
  u8: Uint8Array;
}

export class Sim {
  private constructor(private x: Exports) {}

  static async load(wasm: ArrayBuffer | Response, world: Uint8Array | null, testCohorts = 0): Promise<Sim> {
    const bytes = wasm instanceof Response ? await wasm.arrayBuffer() : wasm;
    const module = await WebAssembly.compile(bytes);
    const instance = await WebAssembly.instantiate(module, {});
    const x = instance.exports as unknown as Exports;
    if (x.rq_abi_version() !== ABI_VERSION || x.rq_snapshot_len() !== SNAP_LEN) throw new Error("simulation wasm does not match abi.gen.ts; run `bun tools/requiem.ts sim`");
    if (world) {
      const ptr = x.rq_alloc(world.length);
      new Uint8Array(x.memory.buffer, ptr, world.length).set(world);
      const code = x.rq_load(ptr, world.length);
      x.rq_free(ptr, world.length);
      if (code !== 0) throw new Error(`world file rejected (${code})`);
    } else if (testCohorts > 0) {
      x.rq_load_test(testCohorts);
    }
    return new Sim(x);
  }
  reset() {
    this.x.rq_reset();
  }
  tick(buttons: number, lx: number, ly: number, rx: number, ry: number) {
    this.x.rq_tick(buttons, lx, ly, rx, ry);
  }
  tickAuto(): number {
    return this.x.rq_tick_auto();
  }
  /** The current snapshot; valid until the next call. */
  snapshot(): Float32Array {
    return new Float32Array(this.x.memory.buffer, this.x.rq_snapshot(), SNAP_LEN);
  }
  /** The knights inside `planes` (six of `n·p + d >= 0`) within `far` of the eye; views are valid until the next call. */
  crowd(planes: Float32Array, eye: readonly number[], far: number): CrowdView {
    new Float32Array(this.x.memory.buffer, this.x.rq_planes(), 24).set(planes);
    const count = this.x.rq_crowd(eye[0], eye[1], eye[2], far);
    const ptr = this.x.rq_crowd_ptr();
    const buf = this.x.memory.buffer;
    return { count, f: new Float32Array(buf, ptr, (count * DRAW_BYTES) / 4), u16: new Uint16Array(buf, ptr, (count * DRAW_BYTES) / 2), u8: new Uint8Array(buf, ptr, count * DRAW_BYTES) };
  }
  /** The effect slots: `FX_SLOTS` records of `FX_BYTES`. */
  fx(): DataView {
    return new DataView(this.x.memory.buffer, this.x.rq_fx(), FX_SLOTS * FX_BYTES);
  }
  bolts(): DataView {
    return new DataView(this.x.memory.buffer, this.x.rq_bolts(), BOLTS * BOLT_BYTES);
  }
  height(x: number, z: number): number {
    return this.x.rq_height(x, z);
  }
  private mats(ptr: number): Float32Array {
    return new Float32Array(this.x.memory.buffer, ptr, BONES * 12);
  }
  /** A copy of the bind-pose bone transforms of a figure (`FIGURE`). */
  bind(kind: number): Float32Array {
    return this.mats(this.x.rq_bind(kind)).slice();
  }
  /** Skin matrices of a knight at a stored frame, in the figure's frame; valid until the next call. */
  knightFrame(kind: number, frame: number): Float32Array {
    return this.mats(this.x.rq_knight_frame(kind, frame));
  }
  /** Skin matrices of the mage at the origin in a given state; valid until the next call. */
  magePose(show: number, mv: number, t: number, phase = 0, speed = 0, hover = 0): Float32Array {
    return this.mats(this.x.rq_mage_pose(show, mv, t, phase, speed, hover));
  }
  demon(): Float32Array {
    return this.mats(this.x.rq_demon());
  }
  /** Renders `frames` stereo frames of sound; the view is valid until the next call. */
  audio(frames: number, rate: number): Int16Array {
    return new Int16Array(this.x.memory.buffer, this.x.rq_audio(frames, rate), frames * 2);
  }
}
