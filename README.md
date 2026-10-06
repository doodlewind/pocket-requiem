# Pocket Requiem

A crowd-battle action game for the PS Vita, the PSP and the Nintendo 3DS: at night, on a barren field, a mage meets the headless army a demon holds. One mage, a staff, and **4 053 knights** on one seamless field, at **30 frames per second**.

It plays like a crowd-battle action game. □ chains five strikes of the staff; △ after `n` strikes casts the spell of that step; ○ with a full gauge undoes the binding on every knight around her. A strike that lands holds the frame for a few ticks before anything moves again.

The source is public at https://github.com/doodlewind/pocket-requiem, and anyone with a Pocket Studio account can remix the game ([Remixing this game](#remixing-this-game)). The game's packages and its browser version are published on Pocket Studio (https://studio.pocket.nexus), where its members download the packages ([Releases](#releases)).

| | Screen | Renderer | Measured |
| --- | --- | --- | --- |
| PS Vita | 960 × 544, 4× MSAA, bloom, moon shafts, graded composite | GXM, programs compiled on the device | 90 s of the autopilot's fight: 2 700 frames, **0 late**, **296 to 1 750 knights in view** (mean 1 147), up to 328 000 triangles |
| PSP | 480 × 272, 16-bit colour | GE, fixed function | 2 700 frames, **0 late**, **40 to 1 560 knights in view** (mean 986), up to 37 000 triangles |
| Nintendo 3DS (Old) | 400 × 240, the field from above on the lower screen | PICA200, vertex programs | 2 740 frames, **0 late**, **70 to 1 705 knights in view** (mean 1 018), up to 80 000 triangles |

The repository holds the whole path from authoring to hardware:

- **`web/`** is the reference and the place content is authored: a three.js app that generates the field from one seed, builds the models and the effects in code, and runs the game in a browser.
- **`crates/requiem-sim`** is the game: the mage's moves, the army, the freeze a strike causes, the camera, every figure's motion, sound and the autopilot. The reference runs it as wasm; the compiler and the console link it natively. There is one implementation of every rule.
- **`crates/requiem-cook`** compiles the stage for a device profile. It bakes the moonlight into vertex colours, merges the field into cells with levels of detail, **samples the knights' motion into stored frames**, lowers the effects to templates and constants, and writes one pack with a compile receipt.
- **`vita/`** draws the pack and runs the simulation at two ticks per frame.
- **`crates/requiem-handheld`** is what the PSP and the 3DS share: the ground built from two grids, the army's draw list and its far ranks, the effects evaluated into vertices, the sky, the interface and the loop round the simulation. **`psp/`** and **`n3ds/`** are the two renderers over it.

- **`wgpu/`** is the game in a browser tab: the same simulation compiled to wasm32, the PS Vita's pack read over HTTP, and the PS Vita's passes drawn with wgpu over WebGPU, in PocketJS's Pocket3D player.

PocketJS (pinned in `vendor/pocketjs`) supplies the device toolchains, the dev hosts, the GXM kernel, packaging, the app icon, and the browser kernel with its player.

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
- **Her arms follow the staff.** A key says where her right hand is, where on the shaft it holds (`Prop.slide`) and which way the staff's head points; the left hand holds below it or goes free. The solver (`anim.rs`) hangs each elbow down her trunk, in the chest's frame, so it follows a twist of the trunk, and turns each hand on the shaft until it continues its forearm. At ease she holds the staff upright, 13 cm above its middle, with the forearm level; running and hovering she carries it at its middle beside her hip with the arm let down; behind the barrier it is level across her chest with a hand before each shoulder.
- **The freeze.** A strike that lands holds the mage and every knight it struck for 3 to 16 ticks; a struck knight keeps its frame and shivers, then flies. The rest of the field moves on. Inputs pressed during the hold are kept.
- **The spells.** △ alone: a beam, a lane 30 m long. After one to four strikes: a rising burst that lifts what stands around her, lightning in a fan, fire as a burst ahead, and a volley of ten homing bolts. ✕ evades; L raises the barrier; R hovers at 15 m/s.
- **The army** takes turns: three knights at most wind up or strike at once; the rest stand off in a loose ring. At most **360 knights are out of formation at once** (`Crowd::free_cap`; a host sets it lower): a cohort that would pass it holds its ranks at the edge of the fight until knights fall. A knight out of formation is what a tick costs, so the cap bounds the tick when the mage stands still and the whole army closes in. A blow costs her 6 to 11 of 1 000 and staggers her when she is not in a move of her own; a knight falls to two strikes of the staff. A struck knight staggers, is driven back, or leaves the ground; one whose binding is undone falls where it stands, and what leaves it rises as a pale flame.
- **The autopilot** plays through the same inputs as a person. It is the attract mode and the repeatable load for measurements.

Transcendentals go through `libm`, so wasm, the host and the Vita compute the same values; `cargo test` runs the autopilot twice and compares.

## Models

`web/src/model/sdf.ts` builds a figure as rounded primitives on the simulation's bind pose, blended into one signed distance field and polygonized with surface nets; skin weights come from the primitives a vertex is nearest to. Trims are cords laid on the field and skinned by it; faces and stripes are flat shapes pressed onto it.

| | Triangles |
| --- | --- |
| The mage | 42 700 |
| The demon | 31 000 |
| A knight, by level | 7 700, 2 060, 704, then 114 and 50 built of boxes |

## Effects: templates and constants

An effect is layers of four shapes (`web/src/fx/ir.ts`): particles, a ring or arc, ribbons that face the eye, a small shell. Each lowers to **a fixed template of vertices and eight rows of constants**. A live effect is one 32-byte record: a place, an age, a direction, one number. The vertex stage places every vertex from the age, so the console stores and steps no particle; all live instances of a layer are one instanced draw. The reference and the Vita evaluate the same formulas (`web/src/render/fx.ts`, `vita/shaders/fx_*.cg`).

Twenty-four effects in 72 layers: the arc of a sweep, the six-petalled circle at the staff's head, the beam, lightning, hellfire, the gold dome of the undoing.

## Compile

```
generators (TypeScript)  →  StageIR  →  requiem-cook --profile vita30   →  the-field.vita30.pack  + .compile.json
                                                      --profile psp30    →  the-field.psp30.pack
                                                      --profile n3ds30   →  the-field.n3ds30.pack
```

`bun tools/requiem.ts cook` exports StageIR (float geometry, the atlas, the models, the lowered effects, the simulation's world file, each file's SHA-256 in a manifest) and runs the compiler. The passes, in order:

1. **bake-lighting**: per vertex, `tint × (moon × N·L × visibility + hemisphere(N) × openness)`, with rays against the ground and one cone per tree.
2. **merge-cells**: a 64 m cell gets a near mesh (4 m ground, full trees) and a middle mesh (8 m ground, simple trees); a 256 m cell gets a far mesh.
3. **quantize**: 16-byte vertices, positions over each mesh's bounds.
4. **bake-crowd**: 3 kinds × 5 levels × 112 frames of placed vertices, 23 MB.
5. **lower-effects**: templates as 12 signed bytes per vertex.
6. **atlas-mips**, **interface-font**, **structural-budgets**.

The Vita pack is 59.6 MB.

The StageIR keeps the ground apart from what stands on it (three ground layers, three prop layers), so a profile decides what the ground becomes. The Vita's lowering merges a cell's ground with its props. The handhelds' lowering (`handheld.rs`) stores **no ground mesh**: the heights are the simulation's grid, which the game holds in memory for its own use, and the baked colours are a second grid of 513 × 513 16-bit entries (`GRND`, 0.5 MB in place of 860 000 triangles). The device builds the patches it draws. The same lowering cuts the mage into draws of the bones one draw can hold (4 on the GE, 19 uniform sets on the PICA), stores the army's frames in the layout each GPU blends, and converts the atlases. The PSP pack is **13.1 MB**, the 3DS pack **18.8 MB**.

## On the PS Vita

- **The army**: two programs. Knights within the first two levels get the moon, the sky, four spell lights, a highlight and a rim per vertex; the far ranks get the moon, the sky and one spell light. On the console the first places about **19 000 triangles a millisecond** and the second **40 000**: the vertex stage is the limit, so the far ranks are few triangles and a short program.
- **A triangle budget.** A frame may draw 230 000 triangles of knights. When the knights in view exceed it, the hand-over distances between levels pull in for that frame (never the distance at which a knight stops being drawn): a press of knights round the eye is drawn a level coarser instead of late.
- **Spell light on the field.** The bake stores tint × light. A mesh within reach of a spell's light draws with a second program that raises the baked colour by `sqrt(1 + cast / reference)` per vertex; the rest draw with the plain one.
- **The freeze, on screen.** While a heavy strike holds the frame the composite hardens the contrast, drains the colour, lifts the bloom and smears the frame toward its centre.
- **Post-processing**: the scene renders to a 960 × 544 target with 4× MSAA; a quarter-size chain keeps what is bright, blurs it and smears it away from the moon; one pass composes and grades.
- **Programs** compile on the device through SceShaccCg on first run and are cached by source hash; the packaged build ships the set a pass on a console collected ([Releases](#releases)).

Measured on a PS Vita (PCH-2000, CPU 444 MHz, GPU 222 MHz), development build in Pocket Devkit, the autopilot fighting (`bun tools/requiem.ts bench --seconds 90`):

| Window | Frames | Late frames | Average frame | Worst frame | Knights in view | Most triangles | Most draws |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 90 s | 2 700 | 0 | 33.37 ms | 33.47 ms | 296 – 1 750, mean 1 147 | 328 066 | 291 |

CPU time per frame: simulation (two ticks) 1.6 ms, the army's draw list and its instanced draws 2.9 ms, all drawing 4.1 ms. GPU time during the fight: 24 to 27 ms of the 33.3.

From a fixed view over the army, at one refresh per frame: sky and post chain 6.8 ms; the field 2.4 ms; the mage 3.3 ms at 62 700 triangles (she is 42 700 now); 700 knights 13.5 to 15.7 ms in all.

## On the PSP and the 3DS

Both run the same pack sections through `crates/requiem-handheld`; a device crate adds its GPU, its pad, its sound and its storage. A machine a tenth as fast draws the same field with four changes of method:

- **The ground is two grids.** A frame walks the 256 m cells: a far one is a patch of 8 × 8 squares, a near one draws its 64 m cells at 16 × 16 squares inside the near distance and 8 × 8 outside. A patch's vertices are written once into a slot (28 near slots, 112 small ones) and stay until a patch that is needed takes the slot of one that no frame has drawn for three frames. Every patch of one size shares one index list.
- **The army has a third rank.** Meshes reach 70 m. Beyond that a cohort still in formation is **one mesh of two quads per knight** (a body that widens to the shoulders, the weapon upright), written where the cohort stands, facing the eye, and drawn with the cohort's march since then as the draw's translation; it is written again after 1.5 m of march, a turn, a lost knight, or 17° of the eye's bearing. A thousand knights are a few dozen draws, and a frame rewrites four cohorts at most.
- **The triangle budget is spent from the eye outward** (`crowd.rs`): the knights are ordered by distance in half-metre steps (inside a step they keep the simulation's order, so two knights a hand apart do not trade levels from frame to frame); as many of the nearest as half the budget pays for at the coarsest mesh are meshes, and **the others are drawn as far figures, not left out**; a quarter of what is left buys the finest level for the front rank, and the rest buys one level at a time, nearest first.
- **Knights out of formation are capped per machine** (`crowd.free` in the profile: 240 on the PSP, 200 on the 3DS). With the mage standing still, the uncapped army put 700 knights round her and the 3DS's tick took 60 ms.
- **The effects are evaluated on the CPU** (`fx.rs`): the Vita's four vertex programs as loops, with what an instance's vertices share computed once and what a particle's corners share once per particle. A device draws two batches: the layers that cover, then the layers that add light. An effect beyond 30 m draws every second particle, beyond 70 m every fourth, and a full buffer leaves out the farthest.

**PSP** (`psp/`, GE fixed function). A knight is one draw of **two morph targets**: the pack stores each frame of the army next to the frame after it (`CRWP`), and the GE blends the pair by the knight's weight; the moon and the sky are baked into each frame's colours. A light a spell casts is a GE point light whose ambient term carries its colour, so knights, ground and props brighten round it without normals. A struck knight's flash is the haze set to a constant share of white for that draw. The ground's squares within 21 m of the eye are tested against the GE's guard band and cut on the CPU. What the CPU computes each frame is written to ordinary memory and flushed, not to the display list's own memory: that is addressed past the data cache, and the interface alone cost 2.6 ms there.

**3DS** (`n3ds/`, C over citro3d, the shared crate behind a C interface). A knight is one draw of two stored frames bound as two buffers and blended by `crowd.v.pica`; the Vita's `CRWD` section is used as it is. Its levels are 2 060, 704 and **442 triangles** (the coarsest cell at which a knight still has two legs and two arms, exported for the handhelds), then the 122-triangle figure of prisms from 44 m: in a press of knights the nearest ninety are smooth meshes. The spells' two strongest lights enter the army's, the figures' and (when one is lit) the ground's vertex programs. The lower screen shows the field from above with the cohorts still in formation.

Measured with the autopilot fighting for 90 s (`bun tools/psp.ts bench`, `bun tools/n3ds.ts bench`):

| Device | Frames | Late | Average | Worst | Knights in view | As meshes | Most triangles | Most draws | CPU: simulation, frame build |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| PSP (333 MHz, over PSPLINK) | 2 700 | 0 | 33.37 ms | 34.18 ms | 40 – 1 560, mean 986 | mean 76, most 84 | 37 076 | 202 | 7.0 ms, 9.7 ms |
| Old 3DS (over the dev wire) | 2 740 | 0 | 33.44 ms | 48.9 ms | 70 – 1 705, mean 1 018 | mean 89, most 120 | 80 255 | 241 | 8.3 ms, 6.7 ms |

The 3DS bench asks the console for its status 18 times, and an answer costs it about one frame; its worst frame is one of those. The 3DS row is build `7d2cc0b037a7`; the prism figure's new shape and the far figures' darker tone came after it and have not been measured there.

With the mage standing still for a minute and the army closed round her (`bun tools/n3ds.ts look --lead 14 --ctl auto=0`, and the same by hand on the PSP): the 3DS holds 33.0 to 33.2 ms with 120 meshes, 260 far figures beside them and the simulation at 7.6 to 13.7 ms; the PSP holds 33.4 ms with 84 meshes and the simulation at 8.2 to 11.0 ms.

The PSP's GE is the limit: with 12 000 triangles of knights' meshes and longer distances it finished 13 of 2 700 frames late, so the profile's distances and the knights' budget (10 500 triangles of meshes) are set below that; far figures are untextured and unlit and cost it little. The PSP uses 17.4 MB after loading. Its CPU needed the simulation 2.8 times faster than it was (18.5 ms a frame to 6.6 ms): knights on their feet step every other tick (every machine shows a frame per two ticks), the neighbour grid is 1 024 cells of 16 bits (the 4 096-cell tables were twice the PSP's data cache, walked every tick), and the synthesizer reads sines and decays from tables. Those changes are in `requiem-sim`, so the Vita's simulation went from 2.4 ms to 1.4 ms a frame.

## In a browser tab

`wgpu/` is a fourth runtime: **`requiem-sim` compiled to wasm32, the PS Vita's pack (`vita30`) as it is, and the PS Vita's programs in WGSL**, drawn with wgpu over WebGPU. The page is PocketJS's Pocket3D player: the game in the shell of a PS Vita, a PSP or a Nintendo 3DS, the shell's keys as the controls, and the way to the game's card in Pocket Studio. The readouts are drawn by the renderer, as on the consoles, and the sound is the simulation's synthesizer.

- **Whatever device the page shows, the pack and the programs are the PS Vita's.** A device changes the screen's size, whether the frame goes through the PS Vita's chain, the cap on knights out of formation, and the buttons. The player says for each device how its own build differs.
- **The fight starts on 25.0 MB of the pack's 59.6 MB**; the detailed meshes of the field and the stored frames of the army's two finest levels are read behind it. Until they arrive a near cell draws its simple mesh and a knight is drawn a level coarser.
- **The same renderer runs on the build machine** (`wgpu/src/bin/shot.rs`), where it writes frames to a file. `bun tools/listing.ts` records the game's listing for Pocket Studio with it.

Measured in Chrome 154 on an M3 Max: 30 frames a second on each device, a frame costing 0.26 ms with 1 654 knights and 321 000 triangles in view; the first frame of the fight 2.4 s after the page's start with the pack at hand, 13.5 s over a line of 16 Mbit/s. [`wgpu/README.md`](wgpu/README.md) has the mechanisms, the differences from the PS Vita's renderer and the measurements.

The three.js reference in `web/` is where the stage is authored. It is not the browser version and is not published.

## In the launcher

The icon in each console's launcher is **the Pocket3D icon, read from the PocketJS checkout** (`vendor/pocketjs/engine/pocket3d/icon/`) when the package is built. This repository holds no icon file; the launcher's title string, "Pocket Requiem", names the game.

| Console | Where the build names the icon | File under `vendor/pocketjs/engine/pocket3d/icon/` |
| --- | --- | --- |
| PSP | `xmb_icon_png` in `psp/Psp.toml` | `psp/ICON0.PNG`, 144 × 80 |
| PS Vita | `icon: POCKET3D_ICON.vita` in `tools/vita.ts`; the packager puts it at `sce_sys/icon0.png` | `vita/icon0.png`, 128 × 128, 8-bit indexed |
| Nintendo 3DS | `ICON` and `SMALL_ICON` in `n3ds/Makefile`, both given to `smdhtool` | `3ds/icon.png`, 48 × 48, and `3ds/icon-small.png`, 24 × 24 |

The pictures beside the icon are captures of this game. `psp/assets/pic1.png` (480 × 272) is the XMB's background. `vita/assets/sce_sys/livearea/contents/bg.png` (840 × 500) and `startup.png` (280 × 158) are the LiveArea's background and gate: `bun tools/livearea.ts` renders tick 900 of the autopilot's fight in the reference at two and four times those sizes, averages each down and writes it with a palette of 256 colours, the form the VPK packager requires.

## Controls

| | Vita | PSP | 3DS | Keyboard |
| --- | --- | --- | --- | --- |
| Strike | □ | □ | Y | J |
| Spell | △ | △ | X | K |
| Evade | ✕ | ✕ | B | Space |
| Undo the binding (full gauge) | ○ | ○ | A | L |
| Barrier (hold) | L | L | L | Q |
| Hover (hold) | R | R | R | Shift |
| Move | left stick | stick | Circle Pad | WASD |
| Camera | right stick | direction pad | +Control Pad or C-Stick | arrows or mouse |
| Autopilot on or off | START | START | START | `?auto` in the URL |
| Start again | SELECT | SELECT | SELECT | Backspace |

## Commands

```
bun run setup                          # submodule and dependencies
bun run dev                            # build the wasm simulation, serve the reference on :5283

bun tools/requiem.ts sim               # wasm + web/src/sim/abi.gen.ts
bun tools/requiem.ts shot --out a.png [--auto --ticks 900] [--view px,py,pz,tx,ty,tz,fov] [--query "test=8&press=2:2"]
bun tools/requiem.ts shot --out m.png --query "model=mage&poses=i:0@0,m1:10@40,m6:18@40"
bun tools/requiem.ts cook [--no-export]
bun tools/livearea.ts [--check]        # the Vita LiveArea pictures from the reference; --check runs the packager's rules on them

# PS Vita
bun tools/requiem.ts serve             # USB host for the console (keep running)
bun tools/requiem.ts native            # sync the pack, build, replace the binary in Pocket Devkit
bun tools/requiem.ts status | capture --out f.png | bench --seconds 90
bun tools/requiem.ts ctl '{"auto":false,"view":{"pos":[0,14,500],"target":[0,0,300],"fov":58}}'
bun tools/requiem.ts vpk | push-vpk    # standalone PKRQ00001 package; send it to ux0:data/pocket-requiem/
bun tools/requiem.ts programs          # the console compiles every program in one run: the set a release carries

# PSP (PSPLINK)
bun tools/requiem.ts cook --profile psp30
bun tools/psp.ts serve                 # usbhostfs_pc with a log (one owns the cable)
bun tools/psp.ts run                   # build, stage on host0:, reset PSPLINK, wait for it, start
bun tools/psp.ts status | capture --out f.png | bench --seconds 90 | ctl "crowd=0 stats=1"
bun tools/psp.ts emu --frames 600 --out f.png   # PPSSPPHeadless, software renderer
bun tools/psp.ts package               # dist/psp/PSP/GAME/PocketRequiem for a Memory Stick

# Nintendo 3DS (.3dsx over the paired LAN wire)
bun tools/requiem.ts cook --profile n3ds30
bun tools/n3ds.ts install              # build in the devkitARM container, send, start
bun tools/n3ds.ts status | capture --out f.png | bench --seconds 90 [--install] | ctl "auto=1"
bun tools/n3ds.ts look --install --lead 14 --ctl "auto=0" --frames 5 --every 11   # one lease: install, run, steer, capture
bun tools/n3ds.ts emu [--seconds 6] [--out f.png]   # the built .3dsx in Azahar

# Packages for Pocket Studio (Releases, below)
bun tools/release.ts [--targets vita,psp,3ds] [--no-build] [--upload]

cargo run --release -p requiem-handheld --example probe -- .pocket-build/stage/the-field.psp30.pack 90
cargo run --release -p requiem-handheld --example siege -- .pocket-build/stage/the-field.n3ds30.pack 120   # she stands still

# A browser tab (wgpu over WebGPU), and frames on this machine's GPU
bun tools/wgpu.ts cook                 # the PS Vita's pack and the 3DS's map of the field
bun tools/wgpu.ts serve                # build, then http://127.0.0.1:8802/
bun tools/wgpu.ts shot --frames 300 --out a.png [--shape psp] [--words "auto=0 view=…"]
bun tools/wgpu.ts check [--dist]       # Chrome: every device from the title card into the fight
bun tools/wgpu.ts dist                 # the directory `pocket-studio site` deploys
bun tools/listing.ts [--upload]        # the listing's clips, stills and share picture → dist/listing/

cargo test --workspace
cargo test --manifest-path wgpu/Cargo.toml
bun test ./tools                       # launcher art, the check on the Vita's programs, the browser page's devices, the listing's words
cargo run --release -p requiem-sim --bin harness -- .pocket-build/stage/ir/stage.rqsw 120
```

PSP and 3DS control words (`Game::control`): `auto hud stats world crowd mage fx govern` take 0 or 1; `lodNear lodMid lodFar repeat option pace` a number; `reset=1`; `view=px,py,pz,tx,ty,tz,fov` or `view=off`.

Vita `ctl` keys: `auto`, `reset`, `view {pos, target, fov}`, `pace` (refreshes per frame), `profile`, `world`, `crowd`, `mage`, `fx`, `crowdScale`, `crowdBudget`, `farFrom`, `lodNear`, `lodMid`, `hud`, `stats`, `post {…}`, `fetch`.

## Releases

`bun tools/release.ts` builds every device's package from the checked-out commit and writes them to `dist/release/`, which Git ignores:

```
bun tools/release.ts [--targets vita,psp,3ds] [--out dist/release] [--vita-gxp DIR] [--no-build] [--upload]
```

| Target | File | Holds |
| --- | --- | --- |
| `vita` | `pocket-requiem-<version>.vpk` | the program, the `vita30` pack and the programs a console compiled |
| `psp` | `pocket-requiem-<version>-psp.zip` | `PSP/GAME/PocketRequiem/` for the root of a Memory Stick: `EBOOT.PBP` and the `psp30` pack |
| `3ds` | `pocket-requiem-<version>.3dsx` | the program, with the `n3ds30` pack in its ROMFS |

**It needs the toolchains of the three device tools** ([Commands](#commands)) and nothing else: the tool builds the simulation's wasm, exports the stage from its seed, and for each target compiles the profile's pack with this commit's compiler and runs the build a developer runs (`tools/requiem.ts vpk`, `tools/psp.ts package`, `tools/n3ds.ts build`). The version is the one in `package.json`. A target that fails is listed with its error, the other targets build, and the exit status is 1. Each target's build output is in `.pocket-build/release/logs/`.

`release.json`, beside the packages, records **the commit, the version, each package's size and SHA-256, the SHA-256 of the exported stage and of each pack, the Vita programs' list and the build that compiled them, and the toolchains**: the pinned PocketJS revision, the `rustc` of each target, VitaSDK's compiler and `version_info.txt`, the PSP SDK's hash and the devkitARM image's digest.

**The Vita's programs are an input, collected by a pass on a console.** SceShaccCg runs on a console, so the package carries `.gxp` files a console compiled. The runtime asks for every program it has while it loads (17 programs, 26 sources: the field's two, the army's two, the figures', the effects' and the post-processing chain's); no setting adds one later, and the multisampling choice changes how a program is patched, not its source. `bun tools/requiem.ts programs` builds the development build from the checkout, starts it in Pocket Devkit with `{"programs": "fresh"}` in `boot.json`, so it reads no program from the card and compiles each one, waits until it runs, and writes the console's list, the `.gxp` files and `coverage.json` to `.pocket-build/vita-programs/`. `coverage.json` records the build's id, the pack's SHA-256, the SHA-256 of `vita/shaders`, and how many programs the build asked for and compiled. **The release tool refuses a set without that record**, and one whose record names other shader sources or another pack than the one it is packaging (`coverageFault` in `tools/vita.ts`, tested by `tools/vita-programs.test.ts`). The numeric `#define`s a program starts with come from the pack's scene record and its army header, which is why the record names the pack. `--vita-gxp DIR` names another directory with the same three kinds of file.

**All three packages are byte-identical across two builds of a commit on one computer.** The tool writes the `.zip` and the `.vpk` itself: entries in the order of their names (in the `.vpk`, `sce_sys/param.sfo` and `eboot.bin` first, as `vita-pack-vpk` has them), every date 1980-01-01, modes 0644 and 0755, deflate at level 6. A development build of the Vita program carries a random build id. For a release the tool names it (`POCKET_RELEASE_BUILD`, which `tools/vita.ts` reads): 32 hex digits from the commit, the `vita30` pack's hash and the hash of the programs' list; `release.json` records it.

**Packages go to Pocket Studio and to no page on GitHub.** `--upload` runs `pocket-studio package <file> --target <id> --version <version>` for each package from the repository's root, where `pocket-studio register --title "Pocket Requiem"` wrote `.pocket-studio.json` (ignored by Git). It refuses a checkout with uncommitted changes. It does not register the game, publish it or change its address; when the link file is missing it prints the commands that write it. `--no-build --upload` sends the packages `release.json` lists, after checking their hashes.

Starting a package without a development link: `bun tools/psp.ts emu --standalone` and `bun tools/n3ds.ts emu` run the built PSP folder and `.3dsx` in PPSSPPHeadless and Azahar, `bun tools/n3ds.ts install --no-build` sends the `.3dsx` to a console over the wire, and `bun tools/requiem.ts push-vpk dist/release/pocket-requiem-<version>.vpk` copies the `.vpk` to `ux0:data/pocket-requiem/` through the running development build, for VitaShell to install.

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
| `crates/requiem-cook` | the compiler: `main.rs` (Vita), `handheld.rs` (PSP, 3DS) |
| `crates/requiem-handheld` | what the PSP and the 3DS share; `examples/probe.rs` runs a handheld pack without a GPU |
| `vita/` | Vita app and its Cg programs |
| `psp/` | PSP app: the GE renderer, the PSPLINK mailbox |
| `n3ds/` | 3DS app: C host and PICA programs (`src/`), the shared crate behind a C interface (`core/`) |
| `profiles/` | compile profiles |
| `wgpu/` | the browser version: the wgpu renderer (`src/render`, `src/shaders`), the shell (`src/app.rs`), the page (`page/`), frames to a file (`src/bin/shot.rs`) |
| `listing/` | the words of the game's listing on Pocket Studio |
| `tools/` | `requiem.ts`, `vita.ts`, `psp.ts`, `n3ds.ts`, `bench.ts`, `shot.ts`, `livearea.ts`, `release.ts`, `wgpu.ts`, `wgpu-check.ts`, `listing.ts` |

## Remixing this game

The source is public, and Pocket Studio lets anyone with an account remix the game: start a game of their own from a copy of it. The game's card in Pocket Studio has **Remix**, which writes a prompt for a coding agent; from a terminal linked to an account it is

```sh
pocket-studio remix requiem
```

It clones this repository (depth 1, `vendor/pocketjs` at its pinned commit) into `./pocket-requiem-remix/` and registers a Pocket Studio project of the remixer's that names this game as the one it came from, in `.pocket-studio.json` there. `bun tools/release.ts --upload`, `bun tools/wgpu.ts dist` with `pocket-studio site`, and `bun tools/listing.ts --upload` then send to that project, not to this game.

**A remix gives itself a name and an identity before it publishes**, so its packages install beside this game's and not over them:

- The title, "Pocket Requiem", and its forms without the space and in lower case.
- The app id `dev.pocket-nexus.requiem`: the Vita's title id and the 3DS's card folder are made from it by PocketJS.
- The Vita title id `PKRQ00001`: nine characters, four capital letters and five digits, its own.
- The folders the game keeps data in on a card: `ux0:data/pocket-requiem`.

They are in `n3ds/Makefile`, `n3ds/src/main.c`, `package.json`, `psp/Cargo.toml`, `psp/Psp.toml`, `psp/src/main.rs`, `tools/listing.ts`, `tools/n3ds.ts`, `tools/psp.ts`, `tools/release.ts`, `tools/requiem.ts`, `tools/vita.ts`, `tools/wgpu.ts`, `vita/Cargo.toml`, `vita/src/main.rs`, `vita/src/paths.rs`, `web/index.html`, `web/package.json`, `web/src/main.ts`, `wgpu/Cargo.toml`, `wgpu/README.md`, `wgpu/page/index.html`, `wgpu/page/main.js`, `wgpu/src/lib.rs`, `wgpu/src/pack.rs`. A web build goes live at the project's own address only once Pocket Nexus has verified the project; packages and the listing need no mark.

## Not done

- **PSP and 3DS, by eye.** Both are measured and captured; nobody has played either. On the PSP most knights within 34 m are the 122-triangle figure of prisms (corners shared so the light rounds them, a collar where the head would be), and two to nine are the 704-triangle mesh: the GE has no room for more. The PSP's mage is 6 158 triangles and the 3DS's 8 484 (42 694 on the Vita). Neither has post-processing; a heavy strike's freeze whitens the frame through the interface.
- The 3DS plays sound through CSND (no DSP firmware dump on the test console); the PSP at 11 kHz. Neither has been heard.
- The PSP has been run from PSPLINK only, on a console with 56 MB free; `bun tools/psp.ts package` writes the Memory Stick layout, and the 17.4 MB it uses fits a 24 MB PSP-1000 on paper.
- Between strikes (the `stance` key) her left hand stops 9 cm short of the staff with the arm straight: the lower shaft is on her right side, past that arm's reach. In the aim of the beam, the lightning and the volley her right forearm lies along the staff, which bends that wrist about 100°.
- **No ending.** The demon stands on her rise with the scales raised (`demon.rs`, `web/src/model/demon.ts`) and kneels when 1 000 knights are undone; the stage shows a line of text and nothing else.
- No grass on the field; the ground is one texture and the bake.
- The sound has not been heard by a person on the console. Hand feel (the freeze lengths, the cancel windows, the camera) is set from the autopilot and from captures, not by play.
- The army's damage and turn-taking are set so that the autopilot does not fall in five minutes of the harness; they have not been tuned by play.
- The reference draws no post-processing; the look of the console's frame is checked on the console.
- **The browser version** reads no gamepad, and has been drawn by Chrome on one machine's GPU and by no other browser. Its PSP and 3DS screens draw the PS Vita's army at those sizes, not those consoles' own.
- The Vita reads the pack whole into memory before it is uploaded; a section-by-section loader would halve the peak.
- The 3DS core links with thin link-time optimization: the full pass fails to load the simulation's bitcode with that toolchain's nightly. `cargo test --release` fails to link the simulation's two binaries for the same family of reason; `cargo test` and `cargo run --release` work.
