# Full handoff: Windows, iOS and Android, 27 September 2026

Eric: *"seems like youre forgetting a couple things that we previously built. I need a full hand off … for Windows, IOS, and Android. Make sure everything is there. All code, build plans, outstanding tasks, full code catalog."*

## 0. What this chat got wrong, and what was done about it

**1. This chat was working on a line that was missing a day of work.**
- Another chat had merged three things onto the same starting point (master `c502f1d`), as branch `friend-ready`:
  - the Atlas Project chat's **26a** (the full code catalogue);
  - its **26b** (Atlas answers instead of replying with documents);
  - **Atlas on a friend's machine**: the model server starts itself, and nobody is called "Eric".
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- **Fixed:**
  - `friend-ready` is merged into this line as **`handoff-0927`**.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
  - The full suite passes on the merged tree (§5).

**2. The install page's `--friends` option ignores the friend channel you already have.**
- Atlas already carries updates to friends itself, through the **release channel over Tor**: `release`, `courier`, `update_apply`, `kin`, `onion`, and doc `UPDATE_COURIER_SPEC.md`.
- `--friends` puts the page on the public internet through Tailscale Funnel instead.
- It's kept, because a friend's *first* install has no Atlas yet to receive anything. But it's named as a ruling for you: `OPEN_GAPS.md` P.8, and D5 in `OUTSTANDING_2026-09-27.md`.

**3. Two documents are both numbered 37.**
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- `37_MERGE_2026-09-27_26b_and_friends.md` (the other chat).
- They cover different things, and both are kept.

## 1. Where everything is

**On your Desktop:** `Atlas Project\6. Handoff 2026-09-27 (Windows, iOS, Android)\`

| folder | what's in it |
|---|---|
| `00_READ_THIS_FIRST.md` | this document |
| *[row removed 28 Sep 2026: trading-system material]* |
| `1_Code\Atlas-source-handoff-0927.zip` | the same tree as files, at `handoff-0927`, for reading without git |
| *[row removed 28 Sep 2026: trading-system material]* |
| `1_Code\SHA256.txt` | checksums |
| *[row removed 28 Sep 2026: trading-system material]* |
| `3_Code_Catalog\` | every module (`CODE_CATALOG`), every public item (`MODULE_REFERENCE`), every test, command and setting, the capabilities list, what's unwired and what's dead, and `FILE_MANIFEST.txt` (SHA-256 of every file in the repo) |
| `4_Platforms\` | `WINDOWS.md`, `IOS.md`, `ANDROID.md`: how each is built, signed and installed, and its state |
| `5_Session_History\` | every session doc (00–38), `NEW_CHAT_START_HERE.md`, and the Atlas Project chat's copies (`docs/improvements-project/`) |
| `6_Builds\` | the signed Android APK (27 Sep) and the friend-ready Windows build (unsigned, x86_64-pc-windows-gnu). The newest iOS and Windows builds are on GitHub (Actions → Artifacts) until you OK the download. |

**Also:**
- **In the laptop's git repo:** `C:\Users\erics\Atlas\atlas-current`, `master` = `handoff-0927`.
- **In both vaults:** `Atlas-Vault\2026-09-26\added-later`.
- **In the Project:** this document.
- **On GitHub:** `ericsnider2003-max/Atlas`, branch `main`. It's personal Atlas only, now including the friend-ready work.

## 2. The key infrastructure, so it isn't forgotten again

Each is a module in `atlas/src/`. `catalogs/CODE_CATALOG_2026-09-27.md` has every one of the 402 modules with its size, state and description.

| system | modules | what it is |
|---|---|---|
| The running Atlas | `daemon`, `crew`, `server`, `doctor`, `roots`, `install` | the loop, the work scheduler, the local server, the health check, one install root |
| The hub (its interface) | `hub`, `hublive`, `dash`, `panel`, `layout_prefs`, `design/hub/` | 30 locked artboards, Warm Paper, phones and accessibility |
| Talking | `voice`, `speech`, `hearing`, `endpoint`, `tts`, `voiceid`, `brain`, `models`, `gguf`, `persona` | wake, listen, the model (llama-server, **now started by Atlas itself**, friend-ready), speak |
| Your devices | `sync`, `hlc`, `household`, `mesh`, `nearby`, `transport`, `phonelink`, `elsewhere`, `wireguard` | conflict-free merge between your devices; Tailscale for the phone; your own server over WireGuard |
| **Friends** | `kin`, `onion`, `wire`, `peerkey`, `friends`, `groups`, `messaging`, `feedback` | end-to-end encrypted Atlas-to-Atlas, **through Tor that Atlas starts itself** |
| **Updates to everyone** | `release`, `courier`, `update_apply`, `upgrade`, `plugins` | one signed release (your ed25519 key); the courier carries it over the friend channel; apply and roll back |
| Phones | `mobile` (the core for iOS and Android), `atlas/mobile/` (Swift, Kotlin, JNI), `ota` (the install page) | see `4_Platforms` |
| Keeping things safe | `vault`, `recovery`, `codes`, `twofactor`, `sealedlog`, `firewall`, `profiles` | the vault, recovery, the personal/business firewall, the sealed activity log |
| Your day | `calendar`, `when`, `mail`, `worklog`, `workday`, `timebox`, `booking` | rounds 9–11 |
| Making things | `craft`, `editcraft`, `motion`, `filmstrip`, `scene3d`, `meshio` | coding, animation, 3-D |
| Building itself | `selfwork`, `selfaudit`, `shakedown`, `build_it`, `improve` | the self-finishing loop |
| *[row removed 28 Sep 2026: trading-system material]* |

## 3. Per platform

The short version is below. The full steps are in `4_Platforms\`.

| | Windows | iPhone and iPad | Android |
|---|---|---|---|
| **Built by** | GitHub, `windows.yml`, with Microsoft's toolchain (MSVC). The laptop can't compile it (1.1). | GitHub, `ios.yml`, on a cloud Mac. | GitHub, `android.yml`, or `atlas/mobile/build-android.sh` on any Linux machine |
| **Latest build** | Windows run 1 (27 Sep), unsigned, `atlas --version` ran on Windows. The friend-ready Windows zip (x86_64-pc-windows-gnu) is on your Desktop. | iPhone run 5 (27 Sep): signed ad hoc for your iPhone and iPad, valid until 27 Sep 2027 | Android run 1 (27 Sep), unsigned. The 27 Sep APK built by hand is signed with your key. |
| **Signed with** | Azure Artifact Signing, **not set up yet** (D3) | Apple, cloud-managed certificate, through the App Store Connect API key (your four secrets) | your key, on the laptop only: `Atlas\signing\atlas-android.p12`, backed up to `Atlas-Vault\signing` |
| **Installed by** | the installer and `atlas setup`; updates through the courier | `atlas install-page Atlas.ipa`, over your Tailscale | the APK copied to the phone, or `atlas install-page Atlas.apk` |
| **Not proven yet** | a signed build; a restart into a new build (8.2) | an install on a real iPhone (D1) | a run on a real phone (D2) |
| **Its own language model** | yes (llama-server, started by Atlas) | **no** (P.7): it uses the laptop's over Tailscale | **no** (P.7) |

The iPhone build needed five runs; run 5 succeeded. Why each failed is in doc 36.

## 4. What changed in this chat (26–27 Sep)

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- **Personal Atlas on GitHub** as `Atlas` (private), with three workflows: iPhone and iPad, Android, and Windows.
- **The iPhone build is signed.** Apple's side is checked first (`asc_profiles.py`). A failed signing prints what the profiles actually grant. The Rust build is cached, and there's a check-only run.
- **The App Group** is used exactly as Apple registered it: `group.group.com.ericsnider.atlas`.
- **`atlas install-page`** (`ota.rs`) puts a phone build on a page the phone installs from.
- **Android versionCode** comes from the build, so updates install over the old app.
- **The Windows workflow** builds the exe, and signs it once Azure is set up.
- **Build plan B5**: ad hoc now; the Unlisted App Store and a Play testing track later; Android developer verification in 2027.
- **The merge with `friend-ready`** (§0).

## 5. Measured on `handoff-0927`

| what | result |
|---|---|
| personal Atlas, `cargo test --no-fail-fast` | 34 targets, **6,987 passed, 0 failed**. The first full run of the merged tree failed 1 test: the inline test splitter, which a test helper in `ota.rs` tripped. It's fixed, and the affected targets were rerun clean. |
| *[row removed 28 Sep 2026: trading-system material]* |
| GitHub builds | iPhone and iPad run 5 ✔, Android run 1 ✔, Windows run 1 ✔ (unsigned) |

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## 6. What's left

`OUTSTANDING_2026-09-27.md` has everything, each item with what it waits on. The next steps that need you:
1. Start Tailscale on the laptop and turn on HTTPS Certificates. Then the iPhone gets its first install (D1).
2. `atlas release keygen`, at the keyboard (D4).
3. Azure Artifact Signing, for Windows (D3).
4. Rule on friends' first install (D5).

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
