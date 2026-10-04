# Pocket Requiem

A crowd-battle action game for the PS Vita: at night, on a barren field, a mage meets the headless army a demon holds. One mage, a staff, and **4 053 knights** on one seamless field, at **30 frames per second**.

It plays like a crowd-battle action game. □ chains five strikes of the staff; △ after `n` strikes casts the spell of that step; ○ with a full gauge undoes the binding on every knight around her. A strike that lands holds the frame for a few ticks before anything moves again.

This repository is private, and the game is not for distribution.

| | Screen | Renderer | Measured |
| --- | --- | --- | --- |
| PS Vita | 960 × 544, 4× MSAA, bloom, moon shafts, graded composite | GXM, programs compiled on the device | 90 s of the autopilot's fight: 2 700 frames, **0 late**, **512 to 1 785 knights in view** (mean 1 289), up to 337 000 triangles |

The repository holds the whole path from authoring to hardware:

- **`web/`** is the reference and the place content is authored: a three.js app that generates the field from one seed, builds the models and the effects in code, and runs the game in a browser.
- **`crates/requiem-sim`** is the game: the mage's moves, the army, the freeze a strike causes, the camera, every figure's motion, sound and the autopilot. The reference runs it as wasm; the compiler and the console link it natively. There is one implementation of every rule.
- **`crates/requiem-cook`** compiles the stage for a device profile. It bakes the moonlight into vertex colours, merges the field into cells with levels of detail, **samples the knights' motion into stored frames**, lowers the effects to templates and constants, and writes one pack with a compile receipt.
- **`vita/`** draws the pack and runs the simulation at two ticks per frame.

PocketJS (pinned in `vendor/pocketjs`) supplies the Vita toolchain, the dev host, the GXM kernel and VPK packaging.

## The army: stored frames instead of skeletons

The army is the load. A skinned knight costs a draw and a set of bone matrices each; 1 500 of them would cost 1 500 draws and the matrix work for 28 bones apiece. The pack removes both.

- **The motion is a function.** `requiem_sim::knight` defines each kind of knight's eleven clips (idle, walk, charge, chop, sweep, stagger, knock-back, airborne, landing, rising, collapse) as functions from time to a pose. A pose is not a rotation per bone: it says where the feet stand, how the trunk leans, and **where the weapon is and which way it points**; the solver places legs and arms by inverse kinematics (`anim.rs`).
- **bake-crowd** samples every clip at its stored frame times (112 frames per kind), skins each of five levels of detail at each frame, and writes every placed vertex as 12 bytes. A knight on the console has no skeleton.
- **A knight is two frames and a blend.** The simulation keeps a knight's clip and time and turns them into two stored frames and a blend between them (`knight::frames`); a change of clip blends from the frame it left to the frame it enters over six ticks.
- **One draw per pair of frames.** A frame of the game sorts the knights in view by (level, kind, first frame, second frame) and issues one instanced GXM draw per run: two vertex streams of stored frames, one of colours, one of 20-byte records (place, heading, blend, flash, size). 1 300 knights go out in about 110 draws, and no uniform changes between them.
- **A knight in formation costs nothing per tick.** Its place is its cohort's anchor plus its slot, computed when a frame asks for it. A cohort lets its knights go when the mage comes within 52 m; the freed knights run steering, separation through a hashed grid, and the turn-taking of who may strike.

## The stage

`web/src/world/stage.ts` builds a field about two kilometres square from one seed: long swells that flatten where the armies meet, a dry stream bed, low hills that close it, conifers to the east and west, far mountains with snow. The ground is a 513 × 513 height grid at 4 m, stored as 16 bits; the renderers draw and the simulation stands on the same two triangles per square.

The army forms up in 133 cohorts, eleven files across and twelve deep, facing south: longswords, halberds and greatswords, with a captain in each. The demon stands on a rise behind them.

It is night. The moon is low in the north, ahead of the mage, drawn large; the sky is cobalt; the field's light is the moon, a hemisphere of sky, and **what the spells cast**.

## The simulation

`requiem_sim::Sim::tick` advances 1/60 s.

- **Moves are data** (`moves.rs`): a length in ticks, the tick each strike lands and its shape (a sector, a lane, a disc), its damage, push and lift, the ticks it freezes the frame, the first tick a buffered input may cancel it, and the move a light or a heavy input leads to. A press waits up to 16 ticks for a move to accept it.
- **The freeze.** A strike that lands holds the mage and every knight it struck for 3 to 16 ticks; a struck knight keeps its frame and shivers, then flies. The rest of the field moves on. Inputs pressed during the hold are kept.
- **The spells.** △ alone: a beam, a lane 30 m long. After one to four strikes: a rising burst that lifts what stands around her, lightning in a fan, fire as a burst ahead, and a volley of ten homing bolts. ✕ evades; L raises the barrier; R hovers at 15 m/s.
- **The army** takes turns: four knights at most wind up or strike at once; the rest stand off in a loose ring. A struck knight staggers, is driven back, or leaves the ground; one whose binding is undone falls where it stands, and what leaves it rises as a pale flame.
- **The autopilot** plays through the same inputs as a person. It is the attract mode and the repeatable load for measurements.

Transcendentals go through `libm`, so wasm, the host and the Vita compute the same values; `cargo test` runs the autopilot twice and compares.

## Models

`web/src/model/sdf.ts` builds a figure as rounded primitives on the simulation's bind pose, blended into one signed distance field and polygonized with surface nets; skin weights come from the primitives a vertex is nearest to. Trims are cords laid on the field and skinned by it; faces and stripes are flat shapes pressed onto it.

| | Triangles |
| --- | --- |
| The mage | 42 700 |
| A knight, by level | 7 700, 2 060, 704, then 114 and 50 built of boxes |

## Effects: templates and constants

An effect is layers of four shapes (`web/src/fx/ir.ts`): particles, a ring or arc, ribbons that face the eye, a small shell. Each lowers to **a fixed template of vertices and eight rows of constants**. A live effect is one 32-byte record: a place, an age, a direction, one number. The vertex stage places every vertex from the age, so the console stores and steps no particle; all live instances of a layer are one instanced draw. The reference and the Vita evaluate the same formulas (`web/src/render/fx.ts`, `vita/shaders/fx_*.cg`).

Twenty-four effects in 72 layers: the arc of a sweep, the six-petalled circle at the staff's head, the beam, lightning, hellfire, the gold dome of the undoing.

## Compile

```
generators (TypeScript)  →  StageIR  →  requiem-cook --profile vita30  →  the-field.vita30.pack + .compile.json
```

`bun tools/requiem.ts cook` exports StageIR (float geometry, the atlas, the models, the lowered effects, the simulation's world file, each file's SHA-256 in a manifest) and runs the compiler. The passes, in order:

1. **bake-lighting**: per vertex, `tint × (moon × N·L × visibility + hemisphere(N) × openness)`, with rays against the ground and one cone per tree.
2. **merge-cells**: a 64 m cell gets a near mesh (4 m ground, full trees) and a middle mesh (8 m ground, simple trees); a 256 m cell gets a far mesh.
3. **quantize**: 16-byte vertices, positions over each mesh's bounds.
4. **bake-crowd**: 3 kinds × 5 levels × 112 frames of placed vertices, 23 MB.
5. **lower-effects**: templates as 12 signed bytes per vertex.
6. **atlas-mips**, **interface-font**, **structural-budgets**.

The pack is 59.6 MB.

## On the PS Vita

- **The army**: two programs. Knights within the first two levels get the moon, the sky, four spell lights, a highlight and a rim per vertex; the far ranks get the moon, the sky and one spell light. On the console the first places about **19 000 triangles a millisecond** and the second **40 000**: the vertex stage is the limit, so the far ranks are few triangles and a short program.
- **A triangle budget.** A frame may draw 230 000 triangles of knights. When the knights in view exceed it, the hand-over distances between levels pull in for that frame (never the distance at which a knight stops being drawn): a press of knights round the eye is drawn a level coarser instead of late.
- **Spell light on the field.** The bake stores tint × light. A mesh within reach of a spell's light draws with a second program that raises the baked colour by `sqrt(1 + cast / reference)` per vertex; the rest draw with the plain one.
- **The freeze, on screen.** While a heavy strike holds the frame the composite hardens the contrast, drains the colour, lifts the bloom and smears the frame toward its centre.
- **Post-processing**: the scene renders to a 960 × 544 target with 4× MSAA; a quarter-size chain keeps what is bright, blurs it and smears it away from the moon; one pass composes and grades.
- **Programs** compile on the device through SceShaccCg on first run and are cached by source hash.

Measured on a PS Vita (PCH-2000, CPU 444 MHz, GPU 222 MHz), development build in Pocket Devkit, the autopilot fighting (`bun tools/requiem.ts bench --seconds 90`):

| Window | Frames | Late frames | Average frame | Worst frame | Knights in view | Most triangles | Most draws |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 90 s | 2 700 | 0 | 33.37 ms | 33.47 ms | 512 – 1 785, mean 1 289 | 336 781 | 321 |

CPU time per frame: simulation (two ticks) 2.4 ms, the army's draw list and its instanced draws 3.7 ms, all drawing 4.9 ms. GPU time during the fight: 24 to 27 ms of the 33.3.

From a fixed view over the army, at one refresh per frame: sky and post chain 6.8 ms; the field 2.4 ms; the mage 3.3 ms at 62 700 triangles (she is 42 700 now); 700 knights 13.5 to 15.7 ms in all.

## Controls

| | Vita | Keyboard |
| --- | --- | --- |
| Strike | □ | J |
| Spell | △ | K |
| Evade | ✕ | Space |
| Undo the binding (full gauge) | ○ | L |
| Barrier (hold) | L | Q |
| Hover (hold) | R | Shift |
| Move | left stick | WASD |
| Camera | right stick | arrows or mouse |
| Autopilot on or off | START | `?auto` in the URL |
| Start again | SELECT | Backspace |

## Commands

```
bun run setup                          # submodule and dependencies
bun run dev                            # build the wasm simulation, serve the reference on :5283

bun tools/requiem.ts sim               # wasm + web/src/sim/abi.gen.ts
bun tools/requiem.ts shot --out a.png [--auto --ticks 900] [--view px,py,pz,tx,ty,tz,fov] [--query "test=8&press=2:2"]
bun tools/requiem.ts shot --out m.png --query "model=mage&poses=i:0@0,m1:10@40,m6:18@40"
bun tools/requiem.ts cook [--no-export]

# PS Vita
bun tools/requiem.ts serve             # USB host for the console (keep running)
bun tools/requiem.ts native            # sync the pack, build, replace the binary in Pocket Devkit
bun tools/requiem.ts status | capture --out f.png | bench --seconds 90
bun tools/requiem.ts ctl '{"auto":false,"view":{"pos":[0,14,500],"target":[0,0,300],"fov":58}}'

cargo test --workspace
cargo run --release -p requiem-sim --bin harness -- .pocket-build/stage/ir/stage.rqsw 120
```

Vita `ctl` keys: `auto`, `reset`, `view {pos, target, fov}`, `pace` (refreshes per frame), `profile`, `world`, `crowd`, `mage`, `fx`, `crowdScale`, `crowdBudget`, `farFrom`, `lodNear`, `lodMid`, `hud`, `stats`, `post {…}`, `fetch`.

## Layout

| Path | Contents |
| --- | --- |
| `web/src/world` | the field: heights, ground, trees, the army's muster (`stage.ts`), atlas painter, scene constants |
| `web/src/model` | the modelling kit (`sdf.ts`), the mage, the knights |
| `web/src/fx` | FxIR, the effects, the effects' atlas |
| `web/src/render`, `web/src/game`, `web/src/main.ts` | reference renderer, input, interface |
| `web/scripts/export-stage.ts` | StageIR export |
| `crates/requiem-sim` | simulation core, wasm interface, snapshot layout, harness |
| `crates/requiem-pack` | pack container and layouts |
| `crates/requiem-cook` | the compiler |
| `vita/` | Vita app and its Cg programs |
| `profiles/` | compile profiles |
| `tools/` | `requiem.ts`, `vita.ts`, `bench.ts`, `shot.ts` |

## Not done

- **PSP and 3DS.** The pack layouts for them are in `requiem-pack`; no profile, lowering or runtime exists yet. The stored frames suit both: the GE blends vertex frames in hardware, and a PICA vertex program can.
- **The demon is posed, not modelled.** `demon.rs` places her; no model is built or drawn, and the stage has no ending beyond a line of text when 1 000 knights are undone.
- No grass on the field; the ground is one texture and the bake.
- The sound has not been heard by a person on the console. Hand feel (the freeze lengths, the cancel windows, the camera) is set from the autopilot and from captures, not by play.
- The autopilot falls about once a minute: the army's damage and turn-taking need play to tune.
- The reference draws no post-processing; the look of the console's frame is checked on the console.
- The pack is read whole into memory before it is uploaded; a section-by-section loader would halve the peak.
