# Outstanding — everything not done, and why

**26 September 2026.** Every item here is named with what it's waiting on.
Nothing is left out because it's awkward. Counts come from the generated
catalogues in `catalogs/` (`DEAD_CAPABILITIES_2026-09-26.md`,
`UNWIRED_2026-09-26.md`, `DEAD_CONFIG_2026-09-26.md`,
`CATALOG_TESTS_2026-09-26.md`), not from memory.

## 1. Waiting on Eric's decision

| item | what's needed |
|---|---|
| **The merge** | Done on 27 Sep as branch `friend-ready` (doc 37): 26a and 26b on master `c502f1d`, nothing lost (3,212 rows checked). Eric takes it with a fast-forward on the laptop. |
| *[row removed 28 Sep 2026: trading-system material]* |
| **H2 — panels and the desktop overlay** | Ruled to wait for the merge or the hub. |
| **H7 — the phone calendar** | Ruled to wait for the merge or the hub. |
| **H13f — trying voices before choosing** | Ruled to wait for the hub. |
| **K — the hub pages** | The hub is paused, as Eric asked. |
| *[row removed 28 Sep 2026: trading-system material]* |

## 1b. From the answers fix (26b)

| item | what's needed |
|---|---|
| The friend's machine | Not seen. Install 26b there with the 4B model (`atlas get pictures`) and llama-server, and ask it a few questions. |
| Starting llama-server on a fresh machine | Checked on 27 Sep: it never did. Fixed (doc 37 §4) and tested with a stand-in; not yet seen with the real llama-server on a fresh Windows. |
| The merged line (`Atlas\\atlas-current`) | Merged on 27 Sep (doc 37). |

## 2. Waiting on the laptop (built, tested with mocks, not yet run for real)

Live-test install: `C:\Users\erics\AtlasLiveTest-0925` (25k/26a program;
25h kept in `previous-25h\`). `C:\Users\erics\Atlas\atlas-current` untouched.

| item | status | what the real run needs |
|---|---|---|
| Push-to-talk, end to end | Key proven on the laptop (hold seen, tap passes through); stops at "I can't hear on this machine yet" | Speech tools (whisper, piper and their models) in the install folder — `atlas doctor` names them |
| Setting a key by pressing it in Settings | Unit-tested; not clicked through | Open Atlas → Settings → Keys → "Set by pressing" |
| Reading your own PDFs and zips, with a real Windows Defender scan | Tested on eight real PDFs/zips/docx here; Defender mocked | One of Eric's files, on the laptop |
| Typing correction in other apps (H4) | Mock-tested | A few minutes typing in Notepad/browser |
| The camera: "look at my screen", faces/things/hands, teaching a gesture (H6) | Models installed on the laptop; not run with the camera | Eric in front of the camera |
| A real call (call notes) | Channel check ran; both silent | A real call |
| Mail sorting (G1) and a scheduled post (G2) | Mock-tested | Eric's real mailbox and a site login in Atlas's browser |
| Two-factor sign-in in Atlas's own browser | Tested in a real headless browser here | Eric's real accounts |
| Robot checks during sign-in | Atlas's browser is hidden, so you can't click a robot check in it; it hands you the page instead | A visible browser window (not built) |

## 3. Ignored tests (4)

- Two in `tests/the_ears_and_the_voice_actually_run.rs`: they need
  whisper-cli, piper, a model and a voice on the machine running the tests,
  and run on a machine that has them.
- Two are examples in doc comments (`reach::Post::quality`,
  `whichone`) marked not to be compiled; they illustrate, they don't test.

## 4. The unused-code backlog (measured by the guards)

| list | count | meaning |
|---|---:|---|
| `KNOWN` / `TEST_ONLY_MAX` | 109 | public functions with a test and no caller — capacity built ahead of a caller or a ruling |
| `ORPHANS` | 5 | no caller and no test — each named with what it waits on |
| `HELPER_UNTESTED_MAX` | 4 | module-internal helpers with no direct test |
| `NO_DAEMON_TEST` | 10 | intents the daemon dispatches that no test drives through it |
| Whole modules nothing reaches | 0 | (ceiling 3) |
| Public surfaces nothing calls | 1 | `look` |

The names are in `catalogs/DEAD_CAPABILITIES_2026-09-26.md` and `UNWIRED_2026-09-26.md`;
the settings that change nothing are in `DEAD_CONFIG_2026-09-26.md`. Every
list fails the build in both directions: it may shrink, and it may grow only
by name.

## 5. Known limits, named in the session docs

- Atlas's browser runs hidden (robot checks, above).
- Push-to-talk and the typing box need Windows; elsewhere there are no global keys and Atlas says so.
- Setting a key by pressing it can't capture Caps Lock or the Windows key alone in the Settings window — type the name or say it.
- A new key takes hold only when Atlas restarts (Windows is handed keys once).
- Names and street addresses are not scrubbed from kept model examples.
- With no voiceprint enrolled, anyone at the unlocked laptop is taken to be Eric.
- A fresh Atlas's first tick takes about 0.9 s.

## 6. Builds

| build | where | state |
|---|---|---|
| Windows `atlas.exe` (personal Atlas) | `builds/Atlas-for-Windows-26a.zip` in this handoff | Release build, x86_64-pc-windows-gnu, no warnings; installed and running on the laptop |
| *[row removed 28 Sep 2026: trading-system material]* |
| The meaning encoder | `embed/dist` | Unchanged since 22 Sep |

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
