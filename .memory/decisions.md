# Decisions

One line each: `- YYYY-MM-DD | key | value`. The newest line for a key wins. These are loaded into every session, so keep them short and true.

- 2026-10-11 | !repo scope | This repo is personal Atlas only. The trading-system Atlas and its history left on 2026-09-28; atlas/tests/personal_atlas_is_its_own.rs keeps it out (source: NEW_CHAT_START_HERE.md)
- 2026-10-11 | !trust order | Read SOURCE_OF_TRUTH.md before trusting any other doc; never leave two things claiming to be the current code (source: SOURCE_OF_TRUTH.md)
- 2026-10-11 | !is it green | Run ./verify.sh; never write a passing-test count into a handoff without running it (source: verify.sh header)
- 2026-10-11 | control system | The owner's control system is outside this repo on the laptop: C:\Users\erics\Atlas\START_HERE.md (source: SOURCE_OF_TRUTH.md, 2026-10-03)
- 2026-10-11 | known red tests | Some tests assert POSIX paths and fail on Windows; a few suites are flaky under --quick; a fully green run needs Linux/CI (source: SOURCE_OF_TRUTH.md, 2026-10-03)
- 2026-10-11 | owner machine | Windows laptop; Windows setup is in platforms/WINDOWS.md
