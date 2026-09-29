# Round 7: moving 3-D, and the renderer rebuilt

**24 September 2026.** Eric: *"Can we take a 3D rendering and combine it with animation so I can have moving 3D renderings? I feel like everything you designed can be better."*

It can, and it is. `scene3d` was a still picture and a turntable. Now a scene can have a timeline, and anything in it can move. The renderer underneath was rebuilt so the pictures read as solid objects in light rather than shaded shapes. The in-house render is now checked frame by frame against real Blender 4.2 rendering the same scene. Everything is still in house: no new crates and no model downloads.

## What a moving scene is

It is the same JSON as before, plus a `duration` (and optionally `fps`). Anything can then move:

| what moves | how you say it |
|---|---|
| an object's position, turn, size or colour | `"animate":[{"prop":"at","keys":[[0,[x,y,z]],[1.5,[x,y,z]]],"ease":"bounce"}]` |
| a steady spin | `"spin":[deg/s about x, y, z]` |
| the camera | `"orbit": degrees over the clip`, or keys on `from`, `at`, `fov` |
| how it gets between keys | `linear`, `step`, `ease-in`, `ease-out`, `ease-in-out` (the default), `bounce`, `back`, `elastic` (Penner's easing set) |
| back-to-back moves | several tracks on one property: the track whose keys span the moment decides it |
| repeating | `"loop": true` on a track |

The model is told all of this (`SCENE_SYSTEM`). When you ask for something that moves ("animate…", "spinning", "bouncing"…) and the model hands back a still scene, the scene is sent back to be fixed. Saying "animate a bouncing ball in 3d" goes to the 3-D path, not the SVG one.

The output is:

- every frame;
- `<name>.gif`, ordered-dithered so skies and shading don't band;
- `<name>.mp4` when ffmpeg is present;
- the first frame as `<name>.png`;
- Blender's frames when Blender is installed.

## What makes it look better

**Shapes.** Cone, capsule and torus are new. Every shape can be turned (`rotate`) and sized (`scale`). The torus is found by sphere tracing (Hart 1996); the rest are solved exactly.

**Soft shadows.** The sun has a size (`softness`, in degrees), so shadow edges blur the way real ones do. Measured on one row across a shadow: **1 px** of edge with a point sun, **13 px** with a 12° sun.

**Light from the whole sky.** Nearby objects block it (ambient occlusion). Where they do, the blocking surface lights things faintly in its own colour instead: a rough one-bounce light, so the ground throws some of its colour up onto a ball.

**Shine.** A highlight, and reflections weighted by angle (Schlick's Fresnel).

**Haze and sky.** Haze toward the horizon, and a sky that deepens overhead.

**Film.**

- ACES filmic tone curve (Narkowicz's fit) and sRGB gamma, so bright light rolls off instead of clipping to white.
- Motion blur from a shutter open for half a frame.
- Rays spread over each pixel with Roberts' R2 low-discrepancy sequence, shifted per pixel, so the same number of rays gives far less grain.
- The grain pattern is fixed per pixel, so still parts of a moving scene don't shimmer.

**Speed.** Every core draws, and each object has a bounding sphere for quick misses. The 3-second, 72-frame demo (480×270, 9 rays a pixel, five objects, soft shadows, a torus) took **18 s** here.

**Found on the way:**

- **Two back-to-back moves on one property overrode each other.** Before, the last track always won, so a ball keyed to bounce down and then rise sat still for the first half.
- **The sun's glow in the sky lighting caused speckle on every upward face.** One ray in a hundred happened to point at the sun and lit the whole pixel. The sky lighting now leaves the glow out.

## Measured (tests/round7.rs, 11 tests)

| test | result |
|---|---|
| a ball keyed across the frame | drawn where its keys put it on every frame, **worst 0.7 px off**. The expected spot is worked out in the test with its own pinhole-camera maths, not by asking the renderer. |
| a spinning box | its silhouette narrows as it turns: 36 → 35 → 30 → 20 px |
| a bounce | falls, bounces back up, and rests where it was keyed (58 vs 59 px) |
| motion blur | a fast ball's streak widens from 20 to 26 columns (travel in half a frame ≈ 8.8 px); with the ball removed, the open shutter changes **no pixel** |
| soft shadows | 1 px of edge with a point sun → 13 px with a 12° sun |
| a moving scene to files | 18 frames, every one different from the last; ffprobe reads the GIF and the MP4 as 160×96, 18 frames |
| checks | a ball keyed out of shot: "out of the picture from about 0.6 s to 1.9 s"; a 2 s scene with nothing animated: refused |
| Blender script | baked frame by frame; compiles; the frame count and motion blur carry over |
| **Blender, actually rendered** | Blender 4.2 (the official `bpy` module, Cycles on the CPU) rendered the animated scene. The ball's position, Atlas vs Blender, frame by frame: **44/44 46/46 55/55 79/79 104/104 113/113 px, worst 0.1 px apart.** |
| GIF dithering | on a gradient with more shades than a GIF can hold, seen in 8×8 blocks: 1.126 levels off plain, 0.952 dithered |
| "animate a bouncing ball in 3d" | the model's first draft was still, so it was sent back once; the second came out animated, as a GIF and an MP4 |

The Blender path is no longer "runs-verifiable only". Real Blender ran the script Atlas writes, for a still and for an animation, and put things where Atlas does.

## Honest limits

- **Blender on the laptop.** It isn't installed there. The script was run against Blender 4.2's own Python module in the cloud workspace; a desktop Blender 4.x or 5.x should behave the same, but hasn't been run.
- **Materials.** Each object is one colour with a shine: no textures, glass or emission. The ground's checker is the only pattern.
- **Shapes are primitives.** No imported models (OBJ/glTF). That's the next step if you want real objects.
- **Draft quality still shows grain** on large flat faces. Good and Best are clean.
- **Whether it looks good** is still yours to judge. What's checked is that it drew, stays in view and moves as keyed.

## Gate

| check | result |
|---|---|
| personal Atlas, all 29 debug targets | **6,423 passed, 0 failed.** The one first-run failure was a name collision: the new renderer's `to_screen` made the unused `gaze::to_screen` look called. It's renamed, and the target reran green. |
| round-7 tests on their own | 11/11 (plus round 6's 8, all still passing) |
| *[row removed 28 Sep 2026: trading-system material]* |
| demo | 72 frames at 480×270: **18 s** in house, **351 s** in Blender 4.2 Cycles for the same scene (`docs/live/round7/`) |
