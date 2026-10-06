# Pocket Requiem in a browser tab

The game of the handheld builds, drawn with [wgpu](https://wgpu.rs) 25: over WebGPU in a tab (wasm32), and over Metal on the machine that builds it, where a frame goes to a file. It runs `requiem-sim`, the crate every device links, reads the PS Vita's pack (`profiles/vita30.json`) as it is and draws the PS Vita's passes. The page shows it as one of three handhelds (PS Vita, PSP, Nintendo 3DS): its screens, its buttons, its readouts at its size. `web/` is something else: the three.js reference the stage is authored in, which is not published.

| Part | What it is |
| --- | --- |
| `src/render/` | The renderer: `vita/src` in wgpu. `world.rs` the field's meshes and the choice of them, `crowd.rs` the army as instanced blends of stored frames, `figures.rs` the sky, the shadows, the mage and the demon, `fx.rs` the effects, `post.rs` the quarter-size chain and the grade. |
| `src/shaders/` | The PS Vita's programs (`vita/shaders/*.cg`) in WGSL, one file a family. |
| `src/hud.rs` | The readouts' batch and its draw: quads and glyphs from the pack's `FONT` section. |
| `src/pack.rs` | The pack over HTTP: the section table, the glyphs, what a first frame needs, then the rest behind the fight. |
| `src/app.rs` | The shell: the screens (`SHAPES`), a handheld's pad, the ticks of a frame, the eye, what the readouts say, the map for a second screen, the sound, the words a development host sends. |
| `src/web.rs`, `page/` | The tab: what the page calls, and the page itself. |
| `src/bin/shot.rs` | Frames on this machine's GPU, written to a PNG or handed to an encoder as rows of RGBA. |
| `vendor/pocketjs/devices/web/pocket-web-wgpu` | PocketJS's browser kernel: what is not this game's. The device and the screens, ranges of a pack, and the page: the title card first, the frame loop, and **the Pocket3D player** (the bar, each handheld's shell with its keys as the controls, the way to Pocket Studio). |
| `tools/wgpu.ts`, `tools/wgpu-check.ts` | cook, build, serve, dist, shot, check. |
| `tools/listing.ts`, `listing/listing.json` | The game's listing on Pocket Studio: its words, and the clips and stills recorded with `shot.rs`. |

```
bun tools/wgpu.ts cook               # the PS Vita's pack, and the 3DS's for its map → .pocket-build/stage/
bun tools/wgpu.ts serve              # build, then http://127.0.0.1:8802/
bun tools/wgpu.ts shot --frames 300 --out a.png [--words "auto=0 hud=0 view=1.5,-0.25,517.6,0,-0.45,520,38"]
bun tools/wgpu.ts check              # Chrome: every device from the title card into the fight, by keys and pointer
bun tools/wgpu.ts dist               # the directory a static host serves
bun tools/wgpu.ts check --dist       # the same check of that directory
bun tools/listing.ts                 # the listing's clips, stills and share picture → dist/listing/
cargo test --manifest-path wgpu/Cargo.toml
```

The build needs the `wasm32-unknown-unknown` target and `wasm-bindgen` 0.2.126 on the path (the version in `Cargo.lock`; `build` refuses another). The kernel's page modules and the title card are staged by PocketJS's own tool (`stagePocket3dWeb` of `vendor/pocketjs/tools/pocket3d-web.ts`); the realm of a PocketJS guest and the UI core it stages are deleted from the site, because this page starts no guest.

## The launch

**The page speaks English and Japanese**, as PocketJS's player does (its README, "Languages"): `main.js` gives the game's sentence, each device's `note` and its own lines to the player as `{ en, ja }`; the player picks the language and shows the game's English for a word with no Japanese.

The page plays the Pocket3D title card first (`playTitle()` of PocketJS's `pocket3d-title`, copied into the site as it is): 144 ticks, 2.4 s, over the whole page. While it plays the page opens the renderer on the canvas and starts reading the pack. When the card has ended the canvas is shown. Until the first set of reads has arrived a frame is the game's name and how much of that set has been read (`App::wait`), drawn with the pack's own glyphs, which are read first; a start that fails is said the same way. A browser without WebGPU is told so in one sentence after the card; there is no other renderer.

## The pack over HTTP

The PS Vita reads its pack whole before it draws: 59.6 MB. **A tab starts the fight on 25.0 MB of it and reads the other 34.6 MB behind the fight** (`src/pack.rs`). The reads are ranges of 1 MiB of the kernel's `Source`, four side by side, in this order:

1. The section table, then the `FONT` section: the canvas says what is read with the pack's own glyphs.
2. The `MESH` table and the head of `CRWD`: they say which bytes can wait.
3. **The first set**: every read that holds anything a first frame needs. When it has arrived the pack goes to the GPU and the fight starts.
4. **The second set**: the reads that lie inside the vertices and indices of the field's detailed meshes (23.1 MB of the pack) or inside the stored frames of the army's two finest levels of detail (19.8 MB). A frame takes two of them at most to the GPU as they arrive (`Renderer::arrived`).

Until its detailed mesh is here **a cell within the near distance draws its simple mesh**, and until a level's frames are here **a knight is drawn as the next coarser level that is** (`World::arrive`, `Crowd::arrive`). The picture changes when a read completes a mesh or a level; nothing else waits for it.

The source has two forms:

- **The pack's file**, on a server that answers byte ranges: each read is a request with a `Range` header. `serve` does this.
- **The pack cut into pieces of one size** with a manifest that lists them (`<meta name="pocket-pack">` names a `.json`). A read fetches the piece it is, whole and with a plain request. `dist` writes this form, with pieces of the size of a read.

`bun tools/wgpu.ts dist` writes `.pocket-build/wgpu/dist` for a host that limits a file to 32 MiB and keeps a file ten minutes in a browser's cache (Pocket Studio's site deployments): `index.html` and `icon.png`; everything else of the site under `app/<build>/`, named by a hash of its contents (the module, the page's scripts and stylesheets, the handhelds' shells and the player's font, the map); `pack/<hash>.json` and the pieces, each named by its own hash. The page alone is asked for again at every visit. With pieces of 1 MiB the directory is **85 files and 61.1 MB**: 57 pieces and their manifest (59.6 MB), 25 files of the site (1.4 MB), the page and the icon. `dist` refuses a directory the host would (a file over 32 MiB, more than 4 000 files or 1 GiB, a top-level `play/` or `runtime/`). A checkout that `pocket-studio register` has linked (`.pocket-studio.json`, which Git ignores) gets the project's id and the Studio's origin written into the page (`<meta name="pocket-app">`, `<meta name="pocket-studio">`): the player's door then leads to the game's card. Nothing uploads the directory: `pocket-studio site .pocket-build/wgpu/dist` does.

## The devices

The page shows one handheld at a time. A device is the renderer's shape (`SHAPES` in `src/app.rs`) and the player's shell of the same name.

| | Scene | The frame | Knights out of formation | Second screen |
| --- | --- | --- | --- | --- |
| PS Vita | 960 × 544, four samples a pixel | the chain: bloom, moon shafts, grade, the smear at speed | 360 | |
| PSP | 480 × 272, four samples a pixel | as drawn; a heavy strike's freeze whitens it | 240 | |
| Nintendo 3DS | 400 × 240, four samples a pixel | as drawn; a heavy strike's freeze whitens it | 200 | 320 × 240: the field from above, and the fight in numbers |

Every device runs at 30 frames a second, two ticks a frame.

**Whatever the device, the pack and the programs are the PS Vita's.** A device changes the screen's size, whether the frame goes through the chain, the cap on knights out of formation (`crowd.free` of that handheld's profile), the labels of the controls in the title's line, and what turns the eye. The PSP's and the 3DS's own renderers (`psp/`, `n3ds/`) draw another army: figures of prisms, far ranks of two quads a knight, hard edges. **Each device says so** (`note` in `DEVICES` of `page/main.js`): the mark **Simulated** in the player's bar shows the player's sentence, then that one.

**The 3DS's lower screen** is a canvas of its own. Its left 240 columns are the `MAPT` section of the 3DS's pack (`cook` writes it to `.pocket-build/stage/the-field.map`, the page reads it beside the pack) with the marks the console draws: a cohort still in formation, the demon, the mage and her heading (`App::lower`). Beside it the page writes the lines the console prints: health, mana, the count, the knights standing, who plays. It is drawn six times a second.

Picking another device in a fight changes the shell and the screen's shape and leaves the fight as it is.

**The keys are the device's buttons, and so are the shell's own**: a key, a d-pad's arm, a shoulder key or a stick on the picture takes a pointer or a finger.

| | Keys | In the game |
| --- | --- | --- |
| The stick (the left one of two) | W A S D | move |
| The right stick of a PS Vita | I J K L | the eye |
| The d-pad | the arrows | the eye, on every device |
| The face button at the left (□, or Y) | C; J on a device with one stick | strike |
| At the top (△, or X) | V; I | spell |
| At the bottom (✕, or B) | X or Backspace; K | evade |
| At the right (○, or A) | Z or Enter; L | undo the binding, with a full gauge |
| L, R | Q, E | barrier, hover |
| START, SELECT | Space (or Escape), Shift | the autopilot on or off; start again |

The autopilot plays from the start, as on a console; a button that plays takes the pad from it. A PS Vita turns the eye with its right stick alone; this page lets the d-pad turn it there too.

## The readouts

**The renderer draws the readouts, as on the consoles**: health and mana, the count and its goal, the chain of hits, the notes, the autopilot's lines. They are `draw_hud` of `vita/src/main.rs`, laid out on 960 × 544 and placed on the screen shown (`readouts` in `src/app.rs`). The glyphs are the PS Vita pack's (`FONT`: 18, 26 and 44 pixels at 960 × 544); a smaller screen draws them at its height over 544, from an atlas with three levels below its own. The game has no PocketJS interface, and the page starts no guest.

## Sound

The simulation's synthesizer (`requiem_sim::audio::Synth`) is the sound of every device. The page asks it for the frames that keep 160 ms queued at the browser's own rate and schedules each answer as a buffer at the end of the one before (`createSound` in `page/main.js`). A browser lets a page sound after a key or a pointer has gone down on it, so the sound starts at the first press. `?sound=off` leaves it out.

## The frame

1. The pad: the buttons and sticks the player's controls hold, as the simulation's input.
2. One tick of the simulation per sixtieth of a second since the last frame, two a frame at 30 frames a second, four at most. On a display whose refresh is no whole number of sixtieths the part of a tick left over is owed to the next frame.
3. What the frame draws, chosen on the processor: the meshes in view and those a spell lights, the knights in view ordered by what they show, the live effects, the shadows' discs, the bones' rows.
4. The scene's pass into a target with four samples a pixel: the sky's dome, the stars, the moon; the field; the shadows; the army; the mage and the demon; the effects that cover, then the ones that add light.
5. The chain at a quarter of the size: what is bright, a blur across and down, the shafts toward the moon.
6. One pass onto the screen: the composite and the grade, then the readouts.

What differs from the PS Vita's renderer:

- **The atlas is decoded on the processor.** The pack holds BC1 blocks; not every WebGPU device samples them. The texels are the same.
- **Sixteen-bit attributes are read as whole numbers and scaled in the program**: positions of the field, texture coordinates, the army's stored frames, a knight's heading. wgpu's Metal backend reads normalized 16-bit pairs in another order.
- **A mesh's bounds are a record read at the rate of instances**, and a draw names its mesh as its instance. The PS Vita folds the bounds into one matrix a draw.
- **Colours are computed in 32-bit floats.** The PS Vita's fragment programs compute in 16.
- **The composite is one program** whose gains are zero where the PS Vita picks another of three.
- **A power of what is left of an effect's life is taken of 0.00001 at least.**
- **The fight starts before the pack is whole** (above). The PS Vita has every mesh and every level from its first frame.
- **The browser compiles the programs** at every load: there is no SceShaccCg and no cache of programs.

## Measured

Chrome 154 (headless, WebGPU on the Apple GPU through Metal) on an M3 Max, the pack `vita30`, served from the same machine (`bun tools/wgpu.ts check --dist`).

| | PS Vita | PSP | Nintendo 3DS |
| --- | --- | --- | --- |
| Frames a second over 4 s | 30.0 | 30.0 | 30.0 |
| A frame's cost without the display, over 120 frames | 0.26 ms | 0.21 ms | 0.23 ms |
| The frame measured, 12 s into the autopilot's fight | 1 654 knights, 321 000 triangles, 240 draws | the same | 1 629 knights, 318 000 triangles, 233 draws |
| The first frame of the fight after the page's start | 2.4 s | 2.5 s | 2.5 s |

- **The first frame of the fight is the end of the title card when the pack is at hand**: 2.4 s. **Over a line of 16 Mbit/s with 40 ms of latency (Chrome's own throttle) and nothing cached it is at 13.5 s**, when the first set (25.0 MB) has arrived, and the whole pack is on the GPU at 31.5 s. Read whole before the first frame, as before the two sets, the fight started at 32.2 s.
- **With the mage standing still for 30 s and the army closed round her** (`auto=0` from tick 840): 243 knights out of formation, 1 722 in view, 312 000 triangles in 339 draws, the hand-over distances pulled in to 0.30 by the PS Vita's budget of 230 000 triangles; a frame costs 0.29 ms, of which the simulation's two ticks 0.09 ms.
- **The pack on the GPU is 63.5 MB**: the pack's sections as they are, and the atlas as RGBA.
- **The site**: the module is 772 750 bytes (262 557 gzipped); 1.4 MB with the player's shells and font and the map.

`check` drives the real page with real input: on each device the title card, the pack read, the fight; a key takes the pad from the autopilot, the stick moves her, START hands the pad back, a key of the shell under the pointer takes it again; the synthesizer answers with sound; the console has no error. Then the mage close up, the army closed round her, another device picked from the bar in the fight, the 3DS's lower screen, a phone's window with a thumb on the shell's key, a browser without WebGPU, and the slow line: the fight starts before the pack is whole, and every mesh and level is there when it is.

## Not done

- **The first set is 25.0 MB.** The simple meshes, the far meshes and the army's three coarser levels are 12 MB of it; reads of 1 MiB that straddle a part that could wait bring the rest.
- No other browser, no other GPU and no phone itself has drawn it. The sound has been checked for being there, not heard by a person.
- No gamepad. The page reads keys, and the shell's keys under a pointer or a finger.
- **Two runs of `shot.rs` are not the same bytes.** The simulation and what a frame draws are the same; in about one frame in a hundred the stars' pixels differ between two runs by up to 12 of 255. The cause is not found.
- The readouts on the 3DS's screen are the PS Vita's glyphs at 0.44 of their size; the console's own are cut for its screen.
