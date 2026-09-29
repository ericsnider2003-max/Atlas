# Round 6: the lost source put back, the rendering arms built, and a second look that didn't make the cut

**24 September 2026.** Eric's instruction: *"Go for another round."*

## 1. The "lost" source wasn't lost. It's in the durable repo now.

The main chat's `MASTER_BUILD_PLAN.md` (commit 952cf93 in `~/Atlas/atlas-current`) marks B2 (the 21 GitHub ports) and B3 (the round-4 closures) as **[REBUILD] "source lost; only atlas.exe survives"**. The source for both, plus round 5, was in this project's round zips and in the cloud workspace. Here is how it went back:

- **The base.** `atlas-current` master (23e + the three hub commits) was cloned from a git bundle made on the laptop.
- **The Atlas crate.** My four Atlas commits (ports round 1, round 3, round 4, round 5) went in with `git am -3`, a 3-way merge against the 23d base they were written on.
- **Conflicts.** There were only three: `MODULES_IN_TREE`, the generated `CAPABILITIES.md`, and `Cargo.toml`.
  - The `Cargo.toml` resolution first dropped the base's three `firstlaunch` Windows features. A line-by-line check caught it, and they were put back.
- **Nothing lost, checked both ways.** Every line the base added since 23d is still present, and every line rounds 1–5 added is present.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- **The merged tree:** 6,377 tests passed. The only two failures were the pinned module count (334 → 339, the base's own five modules) and the regenerated capabilities document. Both are fixed in `68f6b5d`.

Branches, all on top of master `3c87755` (master itself is untouched):

| commit | what |
|---|---|
| `a0e645e` `df33485` `06e1257` `f1c206a` | Atlas: ports round 1, round 3, round 4, round 5 |
| *[row removed 28 Sep 2026: trading-system material]* |
| `68f6b5d` | merge fix-ups (module count, capabilities doc) |
| round 6 (below) | on top |

**For the main chat:** B2 and B3 are **in**, not REBUILD. `MASTER_BUILD_PLAN.md` now says so.

**Two clocks, one kept.** While this round ran, the main chat committed its own Hybrid Logical Clock to master (`3007af9`, `7ec88ee`), wired into `sync` and the new `transport`. Round 1 of the ports had one too, also wired into `sync`. The merge keeps **master's** `hlc.rs` and `sync.rs` whole. Round 1's version is retired, and the one other caller, `yata`, now sorts by master's `sync::effective_stamp`. The round-1 test "a fast clock no longer wins" still passes, now against master's clock.

## 2. B4: animation to GIF and MP4 (built)

`motion` drew and checked SVG animations. A GIF or a video was "deferred": it needs the animation played, meaning sampled at each moment and turned into pixels.

| piece | what it is | source |
|---|---|---|
| `pngcodec.rs` | Reads PNG (8-bit grey/RGB/RGBA/palette, all five filters, Paeth) and writes PNG (stored DEFLATE). It uses the inflater `zipread` already had. | W3C PNG spec; RFC 1950 |
| `gifenc.rs` | Animated GIF89a: median-cut palette (exact colours when there are 256 or fewer), GIF's LZW with variable code width, NETSCAPE loop, and only the changed rectangle stored after frame 1. | GIF89a spec (W3C copy); Heckbert 1982; giflib's code-width rule |
| `filmstrip.rs` | Plays the SVG in Edge or Chrome, headless with a throwaway profile, over the DevTools protocol Atlas already speaks (`cdp`). Per frame it seeks every SMIL and CSS animation to *t*, waits two animation frames, then calls `Page.captureScreenshot`. Output is a GIF, plus an MP4 via ffmpeg when ffmpeg is there. It checks that the result really moves. | Chrome DevTools Protocol; MDN `SVGSVGElement.setCurrentTime`, Web Animations API |

Reached from:

- "animate …" now also saves `animation.gif` (and `.mp4`) beside the SVG.
- `atlas film <animation.svg> [fps]`.

**Measured** (`tests/round6.rs`):

- **PNG, checked against a decoder Atlas didn't write:**
  - Atlas's PNG is read by ffmpeg pixel-for-pixel.
  - ffmpeg's compressed, filtered PNG is read by Atlas pixel-for-pixel.
- **GIF:** 10 frames of a moving square come to 817 bytes (216,000 raw). ffmpeg plays all 10 back pixel-for-pixel.
- **Filmstrip:** 20 frames at 10 fps.
  - The SMIL ball and the CSS square are where the animation says in every frame, worst off by 0.5 px.
  - ffprobe on the MP4 reads 200×100, 20 frames.
- **A fake animation** (an `<animate>` from 40 to 40) passes the source check, but played it gives 8 identical frames. It's flagged "nothing moved".

**Found on the way:**

- **Chrome's one-shot `--screenshot` is unreliable.** In Chromium 141 it captured before the page was fully drawn: a circle missing, a square cut to its top 13 rows. Every frame would have been a guess, which is why frames come over the DevTools protocol instead.
- **Atlas's own HTTP client waited out its timeout on Chrome.** It read until the server closed the connection, and Chrome's debugger endpoint doesn't close. It now stops once `Content-Length` or the last chunk says the reply is whole. This also affected the existing `browser::attach`.
- **Running as root was detected from `$USER`.** It now reads the uid from the kernel.

## 3. B4: the 3-D arm (built)

`scene3d.rs`:

- **The scene.** JSON describing spheres, boxes, upright cylinders, a ground, one sun and a camera. It's what a model drafts well and a person can read.
- **The in-house ray tracer.** Lambert shading with a Blinn–Phong highlight, hard shadows, one bounce of reflection by `shine`, 4 rays per pixel, and linear-light shading so colours come out as asked.
- **Output:** a still PNG and a turntable GIF.
- **The same scene as a Blender script.** It uses bpy primitives, Principled materials, a sun, a tracked camera and Cycles on the CPU, and runs headless (`blender -b --python`) when Blender is installed. That render is checked like every render: it exists, it's a real PNG, and it's the size asked for.

Sources: Shirley, *Ray Tracing in One Weekend* (CC0); Blinn 1977; Blender command-line manual.

Reached from:

- "draw a 3d scene of …" (new intent `scene3d`): the model drafts the scene, with the check's complaints fed back.
- `atlas scene <scene.json>`.

**Measured:**

- 240×160 with a 12-frame turntable in about 3 s (debug build).
- Ball above the box, post to its right, confirmed by pixel position.
- ffmpeg plays the turntable's 12 frames.
- The Blender script compiles as Python: 4 shapes, a sun and a camera.
- A camera turned away draws only sky and says so.

**Honest limit:** Blender isn't in this container and isn't on the laptop, so the Blender path is verified as far as "valid script". The in-house renderer doesn't need Blender.

## 4. Diarization over-splitting: a second look, measured, and kept off

Round 5 left one open item: on a 3-voice call, one of A's turns got a label of its own.

**What was tried.** `diarize::merge_same_voices` pools each label's speech (seconds of it, not one sentence) and merges two labels when one voice explains the pooled speech better than two. That's ΔBIC with full-covariance Gaussians (Chen & Gopalakrishnan 1998).

**How it was measured.** 60 synthetic calls of 2–3 voices with a fan. The penalty was chosen on 40 of them and checked on the 20 it wasn't chosen on.

| | calls exactly right | over-split | two people under one name |
|---|---|---|---|
| choosing calls (40): grouping alone | 34 | 2 | 4 |
| choosing calls: second look, λ = 1.1 | **35** | **1** | 4 |
| held-out calls (20): grouping alone | 19 | 1 | 0 |
| held-out calls: second look, λ = 1.1 | 19 | **0** | **1** |

On the held-out calls it fixed the over-split and put two people under one name once. For meeting notes, splitting one person is the safer mistake. So the second look **ships off by default**: `atlas notes <file> --merge-voices` turns it on. The test pins that choice to the held-out numbers, so if they change, the default has to change with them.

On round 5's own call (A B A C B A), the second look leaves A's third turn on a label of its own: 4 labels for 3 voices, the same as grouping alone. BIC on these short turns doesn't see the two A labels as one voice. So the over-split that was open is **still open**. What's changed is that it's now measured on 60 calls instead of one.

## 5. The Windows machine: tested on the real laptop

Your laptop was reachable this round, so the Windows build was cross-compiled here, copied into `atlas-current\.r6test\`, and run **natively on Windows 11**, not under Wine.

**Rounds 3–6 end-to-end tests on real Windows: 98 of 102 passed.** They included:

- the DPAPI vault opening on your sign-in;
- the keyboard hook starting;
- the machine kept awake;
- `filmstrip` playing an animation in **Edge** over DevTools, with every frame where it should be (worst off 0.5 px);
- the 3-D renderer;
- PNG and GIF read back by your ffmpeg 9.0.1.

The four that failed:

| test | why | now |
|---|---|---|
| `the_blender_script_for_a_scene_is_valid_python` | Windows has a `python3.exe` on the PATH that only opens the Microsoft Store. The test took it for Python. | The test runs it and checks that it works; a placeholder counts as no Python. |
| `the_hand_off_loop_works_a_failing_test_to_a_pass_in_a_copy` | Same placeholder: its check only asked whether `python3` could be started. | Same fix. |
| `doc_without_a_running_atlas_reaches_the_log`, `atlas_fix_on_the_command_line…` | These run `target\debug\atlas.exe`, which a copied-in test binary doesn't have beside it. | Test harness layout, not Atlas. They pass where the suite is built in place. |

**`atlas.exe` itself on the laptop:**

- `atlas scene desk.json` drew the still and the 24-frame turntable.
- `atlas film ..\..\ball.svg` failed: "the browser started but never opened its control port". Chrome itself was fine; run by hand with the same flags, it opened its port in under 8 s. The cause was that the folder was relative. Chrome was handed a relative profile folder and a relative `file://` page, and neither means anything to it. `film` now makes the folder absolute first, and the test now passes a relative folder on purpose.

**The laptop can't build Atlas from source.**

- The active Rust toolchain is `stable-x86_64-pc-windows-gnu`. It needs MinGW's `dlltool.exe`, which isn't installed.
- `cargo test` stops at `parking_lot_core` with "error calling dlltool 'dlltool.exe': program not found". So **ATLAS.bat menu 7 (build and test) cannot work on this laptop as it stands.**
- The MSVC toolchain is installed too, but there's no Visual Studio Build Tools (no linker) either.
- The fix is one install: MinGW-w64 (for example `winget install BrechtSanders.WinLibs.POSIX.UCRT`) or Visual Studio Build Tools with "Desktop development with C++". Until then, Windows builds come from here, cross-compiled.

**Toasts:** the round-5 toast test checks the notification XML. A toast actually appearing in the Action Center was not triggered from here, so as not to pop things on your screen unasked.

## 6. Gate

The whole tree was run after the merge: rounds 1–6 plus the main chat's two newest commits.

| check | result |
|---|---|
| personal Atlas, all 29 debug targets | **6,409 passed, 0 failed, 0 warnings.** The first run failed 7; all 7 are fixed and those targets were rerun green. |
| voice and VAD measurements (release) | voice 5/5, including the 60-call second look (VAD: see LIVE_OUTPUT_R6) |
| *[row removed 28 Sep 2026: trading-system material]* |
| real Windows (laptop) | 98/102 of the rounds 3–6 tests passed; the 4 failures are explained in §5 |

**What the guards found in the main chat's newest code.** They ran for the first time over `3007af9` and `7ec88ee`:

- **Three public functions with no caller:** `hlc::resuming_from`, `sync::clock_at`, `transport::bind_local_ephemeral`. The clock already persists through the log's serde, so the two persistence helpers have nothing to do yet. They're named in `KNOWN` and `ORPHAN_METHODS` with that reason, not deleted, because the chat that wrote them decides. `TEST_ONLY_MAX` went 246 → 249 with the names written beside it.
- **`finding_the_other_machine` broke on master itself.** It reads the "no peers, no broadcast" guard from source, and the transport change reshaped the line to `let (route, peer_addr) = …`. The guard itself is intact. The test now accepts either spelling and still requires "nothing" when there are no peers.
- **The module count** went 343 → 344 (`transport`).

## 7. Still open

- **Blender renders:** Blender isn't installed anywhere Atlas runs yet.
- **A build toolchain on the laptop:** MinGW or Build Tools, as above.
- **Diarization over-splitting:** it stays open. The second look is opt-in because of the held-out result.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
