# Session 27 Sep 2026 (second): Tor ships, friends pass releases on, updates and feedback by voice, and the phone's own model

Started from `handoff-0927` (the 27 Sep handoff, bundle checksum verified). Eric picked all four buildable groups: the friend network, updates and feedback by voice, the installer's Tor and firewall rule, and the phones' own model. Branch **`session-0927b`**.

## 0. Found before starting

**The handoff's own test run wasn't green.** `nothing_from_any_version_was_lost` failed: regenerating the catalogues on 27 Sep removed the nine `catalogs/*_2026-09-26.md` files the lost-work inventory records. The handoff said it passed, so it must have been run before the catalogues were regenerated.
- **Fixed:** the nine are listed as dropped, with the reason (superseded by the 27 Sep files; the 26 Sep copies stay in git and in `Atlas handoff 2026-09-26b\catalogs`).

## 1. What was built

### Friends' network (OPEN_GAPS 8.3, 8.6, 8.7, 8.9, 8.11)

| gap | what it does now | proven by |
|---|---|---|
| **8.3 Tor ships** (closed) | The Tor Project's expert bundle 15.0.23 (tor 0.4.9.12 plus its bridge programs), checked against the Tor Browser Developers key and pinned by SHA-256 from `archive.torproject.org`. The setup window fetches it with the voice pieces, and so does `atlas get tor`. `windows.yml` ships it as `dist/tor/`, checks it, and runs `tor.exe --version` on real Windows. | `tests/tor_ships_with_atlas.rs`: the two pins match; a Tor-shaped `.tar.gz` unpacks into `tor/` (the `unzip`-only systems now use `tar` for it) |
| **8.9 Firewall rule** (built) | The setup's new step, "Letting your own devices reach Atlas", adds one rule through Windows' own prompt, asked once; a no is remembered. The rule covers only `atlas.exe`, incoming TCP, on private and work networks, from your own addresses (this subnet and Tailscale's). | `doorrule` unit tests (the rule's exact arguments, asked once, a no remembered). **Not run on Windows.** |
| **8.6 Networks that block Tor** (closed) | When Tor is stuck for 2 minutes (or says "Problem bootstrapping" three times), Atlas restarts it through the bundle's own bridges: obfs4, then Snowflake, then meek. It remembers the one that got through. When none does, it says so once, goes back to direct, and tries again in 30 minutes. Your own `kin.tor_extra` lines are never switched. | Real tor accepts all three kinds of line and starts `lyrebird` (`conn_done_pt`). The switching, remembering and giving up are tested in `tests/a_network_that_blocks_tor.rs`, with a stand-in tor. |
| **8.7 Speed** (built) | Each friend's Tor connection stays open between messages: at most 16, let go after 4 idle minutes, every request still sealed on its own. The sealed door keeps a connection open when asked. A kept connection the far side closed is replaced once, sealed afresh. | With a SOCKS stand-in for tor: four release pieces over **one** connection, and a dead connection replaced. **Not timed on the real Tor network.** |
| **8.11 Passing releases on** (closed) | A release that arrived and matched is kept to hand on. A device asks whoever answered last, then the releaser, then any friend (at most 3 a tick), and keeps the file only if it matches the fingerprint your key signed. A friend whose pieces made a bad file isn't asked for that file again. A passed-on file stops being handed out once a newer release is heard, or if it failed there. | Real doors over real sockets: Eric's Atlas is off, and Sam's hands the file to Maya. Unit tests cover the order, the cap, a damaged copy, being superseded and a failure. |

### Updates and feedback by voice, and a desktop button (8.2, 8.14)

- **Updates:**
  - "any updates", "install the update".
  - "go back to the last version" asks first: *"Say yes to go back from Atlas X to Y…"*. The yes is the local approval, and it's refused while the laptop is handed over.
- **Feedback:**
  - "report a bug …" reads back **exactly** what will go and sends it only on yes. If an update failed on this device, what was written down about it is attached.
  - "report a bug without the failure …" sends the words alone.
  - "any feedback" lists what's come in.
  - "answer feedback 2 fixing" answers one.
- **Report a problem with Atlas**, in Atlas's own window: write it, then "Show me what will be sent", then "Send it". Editing the text after it's shown means showing it again.
- **Found and fixed:**
  - Spoken replies are cut to a few sentences, so "exactly what will go" wasn't exact. These replies are now read whole.
  - The friend's "Install it now?" question could not be answered, because nothing heard a yes. It now says to say "install the update".
- `tests/updates_and_feedback_by_voice.rs` has 6 tests. The hub pages wait on the hub, which is paused.

### The phones' own language model (D6 / P.7)

**Two root causes, both fixed first:**
1. **The phone core had no model connection at all.** `mobile.rs` started the daemon with `None`: the connection was built only in the desktop program's `main`. So it wasn't just that the phone had no model of its own. It couldn't use the laptop's either. The connection is now in the library (`models::connection`), and `main` and the phone both use it.
2. **Every model call was a `curl` child process**, which iOS forbids and Android can't provide. On phones, `curl` to a plain `http://` address is now done in-process (`tools::curl_in_process`, tested against a real socket).

**Then the model itself** (`phonemodel`, feature `phone-llm`; `phone-llm-metal` for iPhone and iPad):
- llama.cpp is linked in (`llama-cpp-2` 0.1.157, pinned).
- The model is loaded in-process behind the same `brain::Llm`.
- The chat template comes from the model file itself.
- Qwen3's thinking-out-loud is switched off, and any thinking the model does anyway is removed from what it says.
- **"get your own model"** downloads Qwen3 1.7B on phones with 8 GB or more, and 0.6B on the rest. Both are pinned by SHA-256. The download resumes after a break and follows Hugging Face's redirect. **"how's the model download"** says where it's got to.
- Your own `tools.llm` (for example the laptop's model) stays as the fallback.
- **Kept prompts:** Atlas's instructions are about 1,100 tokens and the same every turn, so the model keeps what it has already read. There are two slots, because each turn asks two different things.
  - The first version sent both questions to one slot, and each threw the other's work away: only 358 of 1,096 tokens were reused. Now 1,040 of 1,096 are.

**Measured here (2 laptop CPU cores, no GPU):**

| | load | generation | an answer through Atlas |
|---|---|---|---|
| Qwen3 0.6B (Q8_0, 640 MB) | 0.6-0.9 s | ~16 tokens/s | 12-16 s |
| Qwen3 1.7B (Q8_0, 1.8 GB) | 1.9-4.3 s | ~6.5 tokens/s | 38-59 s |

A phone's GPU should be faster, but that is **not measured**. Answer quality: 1.7B answers sensibly, while 0.6B is weak inside Atlas's long instructions (it once just repeated the question).

**What was built and checked:**
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- **The real download from Hugging Face,** interrupted and then resumed: an ignored network test, run once here and passed.
- **Android:** the core was cross-built with the NDK (r27c) for arm64-v8a with the model inside (`libatlas.a`, 296 MB with symbols).
  - It links into `libatlas_jni.so` exactly as the app's CMake does, with `--no-undefined`. That's 24.5 MB stripped.
  - Nothing new is needed at run time: no `libc++_shared.so`.
  - The full APK wasn't rebuilt here, because there's no Android SDK. `android.yml` will build it.
- **iPhone and iPad:** not built. Only the cloud Mac can build for iOS. `ios.yml` now builds with `phone-llm-metal`, and `project.yml` links Metal, MetalKit, Accelerate and `-lc++`. Its first run is the first compile.

## 2. Measured on `session-0927b`

| what | result |
|---|---|
| personal Atlas, `cargo test --no-fail-fast` | **34 targets, 7,018 passed, 0 failed**, 8 ignored (tests needing things not always here: speech tools, real tor, the internet, doc examples) |
| the phone's engine, `cargo test --features phone-llm --test the_phone_thinks_for_itself` | passed with stories15M, Qwen3 0.6B and Qwen3 1.7B (timings in §1) |
| real tor with the bundle's bridges (`tor_ships … --ignored`, `ATLAS_TOR_BUNDLE`) | passed: all three kinds verify; obfs4 reaches `lyrebird` |
| the real download from Hugging Face, interrupted and resumed (`phonemodel … --ignored`) | passed |
| Android core, `aarch64-linux-android`, `--features phone-llm`, NDK r27c | built; linked into `libatlas_jni.so` with `--no-undefined`; 24.5 MB stripped; the three doors and the JNI entry points exported; needs only liblog, libandroid, libdl, libm, libc |
| Windows, `cargo build --release --target x86_64-pc-windows-gnu` | clean, no warnings; `atlas.exe` sha256 `c96a9d00…a5b03409`; in `Atlas-for-Windows-0927b.zip` with `tor\` beside it |
| *[row removed 28 Sep 2026: trading-system material]* |

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## 3. What's blocked, and why

| what | blocked on |
|---|---|
| Tor between two real networks, speed through real Tor, bridges through a real block (8.4, 8.7, 8.16) | This workspace's network lets no Tor traffic through: real tor started, and every relay and bridge was refused. It needs two real networks. |
| The firewall rule on Windows (8.9), the Windows zip with Tor (`windows.yml` run) | A Windows machine, or a GitHub Actions run. Nothing here pushes to GitHub. |
| The phone model on a phone: speed, memory, battery (P.7) | A real phone. The iPhone build also needs its first `ios.yml` run. |
| A held build is still passed on (new 8.15) | A signed "hold" notice needs the release key (8.1). |
| Updates and Feedback hub pages | Built later the same day, once the hub resumed: doc 40. |

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
