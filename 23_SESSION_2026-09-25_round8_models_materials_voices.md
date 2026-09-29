# Round 8: real objects, real materials, and people who speak once

**25 September 2026.** Eric: *"Everything that can be worked here right now, work. Finish the available gaps and document the rest thoroughly."*

Every gap that could be closed in the cloud workspace was closed and measured. Everything that couldn't is now in **`OPEN_GAPS.md`**, one register saying what each gap is, why it's open, what closes it and who has to act. Branch `round8` holds rounds 6, 7 and 8 plus `master` up to a1b19fb, and fast-forwards from it.

## 1. Models from files: OBJ, STL, glTF/GLB (`meshio`, new)

Before this, a scene could only use primitive shapes. Now any object can be a model file:

```json
{"shape":"mesh", "file":"chair.glb", "fit":0.9, "at":[0,0,0], "rotate":[0,30,0]}
```

- **Read in house**, with no new crates:
  - OBJ with its MTL colours: `v`/`vn`/`f`, negative indices, polygons fanned into triangles.
  - STL, binary or text.
  - glTF 2.0: `.gltf` with its buffers (a file or a base64 `data:` URI) or a single `.glb`. It walks the node tree (matrix or translation/rotation/scale, with children), reads every accessor type (normalized included), with or without indices, and takes `baseColorFactor` as the colour.
- **Fast to hit.** Rays find triangles through a bounding volume hierarchy built by the surface area heuristic (12 bins, Wald 2007 / *PBRT* 4ed §7.3) and Möller–Trumbore intersection. A BVH answer was checked against testing all 3,200 triangles one by one, and they agree exactly.
- **Placed like any shape.** It stands on `at`, centred over it. `fit` sizes its longest side in metres. Rotate, scale, spin and keyframes all work. The file's colours are used unless you give a pattern or material.
- **Found where you'd expect.** A relative file is looked for beside the scene file (`atlas scene look.json`), or in Atlas's `models` folder when Atlas drafted the scene. Drafting also tells the model which files are there, so "put my chair on the desk" can use `chair.glb`.
- **In Blender too.** The Blender script imports the same file (OBJ, STL or glTF importer), joins it into one object, and moves and sizes it exactly the way Atlas does.

The test models are Blender's own Suzanne, exported by Blender 4.2 as OBJ+MTL, STL and GLB (`atlas/docs/live/round8/models/`).

## 2. Materials: glass, glow, patterns

| | how you say it | what it does |
|---|---|---|
| **glass** | `"material":"glass"`, optional `"ior"` (1.5 by default) | bends light by Snell's law and reflects by Fresnel, with total internal reflection; tinted by its colour; a shadow through it lets light through |
| **glow** | `"material":"glow"`, `"glow":` brightness | gives off light in its colour and lights what's around it |
| **patterns** | `"pattern":"checker"` or `{"kind":"stripes","colour":"#223344","size":0.2}` | checker, stripes, grid, dots or noise, fixed to the object so they turn with it |

The model is told all of this (`SCENE_SYSTEM`). Blender gets the same looks: Principled BSDF transmission and IOR for glass, emission for glow, and shader nodes that build checker, stripes, grid and dots with the same arithmetic. Noise uses Blender's own noise, so it's the same kind of pattern, not the same one.

**Glowing things are aimed at directly.** In the first version, glow reached a surface only when a random sky ray happened to hit the lamp. That covered the floor in fireflies, even at 64 rays a pixel. Now each surface picks a direction inside the cone the lamp fills as seen from it (next-event estimation), and weights it by that cone's size. On the lamp scene, the draft's error went from **16.1 to 3.3** levels, and fireflies from hundreds to **1 in 16,000 pixels**.

## 3. A denoiser, and an honest account of what it gained

Dammertz et al.'s edge-avoiding à-trous filter (HPG 2010), steered by each pixel's own noise as in SVGF (Schied et al. 2017):

- The surface colour is divided out and put back afterwards, so patterns and textures stay sharp and only the light is smoothed.
- Neighbours count for less the more their normal, depth or light differs.
- Depth is compared against how fast it changes there, so a floor seen at a low angle still counts as one surface.

**Measured:**

- **Chosen on two scenes:** 3 passes and a tight light test. More passes smeared the steep fall-off around a lamp.
- **Lamp scene:** error 3.31 → 2.91 at draft.
- **Scene it wasn't tuned on** (glass, lamp, model, patterns): 3.19 → 3.01.
- **At "good" quality:** no gain.

So it's on by default for drafts only (`"denoise": true` forces it on). The honest reason the gain is small: most of what's left in a draft is edge aliasing, not grain, and the R2 sampling from round 7 plus aiming at lamps had already removed most of the grain. A new `"quality":"reference"` (64 rays a pixel) is what everything is measured against.

## 4. Checked against Blender 4.2

| what | Atlas vs Blender |
|---|---|
| Suzanne from OBJ, STL and GLB, fitted to 1.4 m and turned 25° | silhouettes overlap **90%+** for each format; the lowest pixel lands on the floor where the test's own pinhole maths puts it |
| checker, stripes, grid, dots on a box face | the same pixel is the same colour of the pattern in **over 90%** of the face, for each |

**Two real bugs were found this way:**

- **The glTF importer turns objects by quaternion**, so the baked Euler angles did nothing. Blender's GLB Suzanne faced forward while Atlas's was turned; the overlap was 72%. The importer's object is now switched to Euler angles, and the overlap went to 95%.
- **Blender 4.2 as a Python module crashed on the way out after the glTF importer**, after the picture was already saved. That made a good render look like a failed one. The script now exits right after rendering.

**Two more found by looking at the pictures:**

- **Pattern flicker.** A box face lying exactly on a square's edge (0.8 m box, 0.2 m squares) flickered between colours from rounding. The pattern is now read just inside the surface.
- **MTL colours.** Blender writes `Kd` as the linear base colour, so Atlas now reads it that way. The OBJ's and the GLB's gold match to 1e-3.

## 5. Who said what: people who speak once, and a count you can give

Round 6 left diarization's over-split open, measured only on calls where everyone speaks at least twice. This round added **60 calls in which, every other time, someone speaks exactly once**. That exposed a bigger failure, the opposite of over-splitting.

**Someone who speaks once gets swallowed by the nearest voice.** Grouping alone put their turn under someone else's name in **17 of 40** calls.

**Tried and dropped: moving a one-turn label to the speaker it fits best.** Held out, it changed nothing, then made things slightly worse. It's removed; the numbers are in the test history and in `OPEN_GAPS.md`.

**Shipped: `split_strangers`, on by default.** Each turn is scored against its speaker's *other* turns (a full-covariance Gaussian over MFCCs). A turn that fits more than a margin worse than the call's median turn gets a name of its own.

| someone-speaks-once calls | right | over-split | mixed |
|---|---|---|---|
| tuning (40): grouping alone | 22 | 1 | 17 |
| tuning (40): split, margin 8 (chosen) | **25** | 5 | **10** |
| **held out (20): grouping alone** | 12 | 1 | 7 |
| **held out (20): split, margin 8** | **15** | **1** | **4** |

Its cost, on the 60 calls where everyone speaks at least twice: right 53 → 50, over-split 3 → 7, mixed 4 → 3. It mixes fewer people in both sets and splits more in one. Round 6 ruled that for notes, splitting one person is the safer mistake. The test ties the default to the held-out numbers: if they stop supporting it, the test fails until the default changes.

**New: `atlas notes call.wav --people 3`.** When you know how many were on the call, that settles the one thing grouping has to guess. Labels merge (the most alike first, by ΔBIC on pooled speech) or split (the worst-fitting turn first) until the count is right. Across all 120 calls: **exactly right 90 → 105**, over-split 13 → 0, mixed 17 → 15.

## 6. Also in this round

- **The main chat's work is merged in.** a1b19fb (a direct peer is listened for every tick) went into `round8` cleanly. Then master moved to **b70be92** (phone-as-peer: tailnet sync, a GUI-free core, a phone platform layer), and that merge is on **`round8m`**. It conflicted only on guard bookkeeping, where both sides had reconciled the same clock and transport. Resolution:
  - `sync` claims `hlc` and `transport`.
  - `platform::mobile` is counted as plumbing: `MODULES_IN_TREE` 346, `UNCLAIMED_MAX` 169.
  - The two methods b70be92 deleted come off `KNOWN` and `ORPHAN_METHODS`: `TEST_ONLY_MAX` 249 → 247.
  - The source-scan test keeps accepting either spelling of the no-peers guard.
- **Run natively on your laptop (Windows 11).** The round-8 tests, cross-built here: **10/10 passed**, with the same numbers to the digit as in the cloud. The Blender comparisons skip there because Blender isn't installed. `atlas.exe scene demo.json` drew the 72-frame demo in **58 s** on the laptop, GIF and MP4. The GIF is byte-for-byte the same size as the cloud's.
- **The guards caught one of mine.** The Blender helper's Python said `addon_utils.enable(`, which made the orphan scanner think `anticipate::enable` had a caller. It's now written `getattr(addon_utils, 'enable')(`, and the count is back to exact.
- **Not wired, on purpose: `sync::clock_at`** (task list #38). It belongs to the main chat's sync work in progress, so it's listed in `OPEN_GAPS.md` §4 rather than wired from here.

## 7. Measured (tests/round8.rs, 10 tests; tests/voice_measured.rs +2)

| test | result |
|---|---|
| the same model from OBJ, STL and GLB | 3,936 triangles each, bounds equal to 1e-4, OBJ and GLB gold equal |
| a glTF written by hand | node tree (translate 10, scale 2, child up 5) lands the triangle at x 10–12, y 10–12; u16 indices; colour read |
| a model where its fit puts it, and Blender agrees | the floor row matches the pinhole maths; overlap with Blender over 90% for OBJ, STL and GLB |
| a glass ball turns the world upside down | left of the ball's middle: **blue** through glass (the right-hand wall), **red** through "glass" with ior 1.0 |
| glass lets light through its shadow | ground brightness under a ball: 150 solid, **226 glass**, 230 in open sun |
| glow lights its neighbours | ground beside a ball at night: 3.2 plain, **36.7** glowing |
| patterns have their size, and Blender agrees | checker 5/5 changes, stripes 0/5, grid 10/10, dots about 30% of the face; Blender agrees on over 90% of pixels |
| the denoiser | 3.31 → 2.91 levels against a 64-ray reference; edges keep 95% of their strength; **1 firefly in 16,000 pixels** |
| a missing model or unknown pattern | said before drawing ("couldn't read no-such-thing.glb", "\"paisley\" isn't a pattern"); drawing still works |
| a scene finds its models beside it | `load_scene` resolves `head.stl` next to `look.json`; `model_files` lists only models |
| the Blender script | imports OBJ, STL and glTF; transmission, emission, pattern nodes; the OBJ keeps its gold |
| someone who speaks once (voice_measured) | the table above; margin 8 is what the tuning calls pick, and the default follows the held-out result |
| told the count (voice_measured) | 90 → 105 of 120 exactly right; every call ends with exactly the count given |

## 8. The demo

`atlas/docs/live/round8/demo.json` is a 3-second orbit at night. Suzanne (the GLB) turns on a striped pedestal, a glass ball rolls in front of a checkered box, a lamp on the floor lights everything near it, and a dotted capsule stands behind. It's 72 frames at 480×270, "good" quality, drawn in **29 s** on this workspace's 2 cores: `demo.gif`, `demo.mp4`, `demo.png`.

## 9. Gate

| check | on `round8` | on `round8m` (+ master's b70be92) |
|---|---|---|
| personal Atlas, all 29 debug targets | **6,437 passed, 0 failed** | **6,441 passed, 0 failed** |
| release-only voice targets (`voice_measured`, `vad_measured`) | 8 passed, 0 failed | 8 passed, 0 failed |
| *[row removed 28 Sep 2026: trading-system material]* |
| round-8 tests on the real laptop (Windows 11, native) | **10/10**, same numbers | — |

Guards are green on both. `MODULES_IN_TREE` 344 → 345 (`meshio`, claimed by `scene3d`) → 346 (`platform::mobile`, plumbing). `CAPABILITIES.md` is regenerated from the built binary.

**Laptop tidy:** with your approval, `atlas-current\.r6test` (367 MB of test bundles, zips and Windows test builds, all already in git) was deleted. `master` and its working tree were not touched.

**For Eric:** `round8m` is the branch to take. `OPEN_GAPS.md` is the list of everything still open, and what each item needs.

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
