# Atlas desktop acceptance handoff — 2026-10-20

## Objective

Use Claude with desktop control to exercise the installed Atlas as a real user,
record every visible defect, and repair only defects that are reproducible and
within the current Foundation Recovery scope.

## Installed candidate

- Executable: `C:\Users\erics\AppData\Local\Atlas\atlas.exe`
- SHA-256: `9DB1759459BF14B8DD3C989A7B89135CC8E921EE141533D8988DFD3806234392`
- Rollback: `C:\Users\erics\AppData\Local\Atlas\atlas.exe.pre-charter-20261020`
- Matching media tools: `C:\Users\erics\AppData\Local\Atlas\tools\ffmpeg\`
- Latest installed real-process verification: `verification/runs/20261020T035302140Z-f566b40c/`
- Result: 14/14 substantive contracts passed.

## What has already been proven without desktop control

The installed executable has passed identity, machine diagnosis, enqueue,
authenticated Hub handoff, clean-path Hub HTTP responses, daemon cold start and
restart liveness, replay after restart, queue replay, approval-gated start,
completion, completed-state replay, and missing-request failure. Source-focused
audio, camera, and video dependency tests also pass with the installed FFmpeg
directory available.

## Desktop test sequence

1. Launch the installed executable and record whether a visible Atlas window,
   tray item, or other user-facing surface appears within 20 seconds.
2. Give Atlas a short typed request: `Create a text file named atlas-test.txt
   in my Desktop folder containing the current date, then verify the file.`
3. Confirm Atlas explains what it understood, performs the work, shows progress
   if it exceeds 20 seconds, and reports the exact output path and verification.
4. Ask a follow-up referring to the created file without repeating its path.
   Record whether context is retained.
5. Start a bounded background request, observe its running state, then restart
   Atlas while it is active. Confirm the request is reconciled as completed,
   failed, or uncertain rather than silently duplicated or lost.
6. Ask Atlas to perform a harmless bounded failure, such as reading a
   deliberately nonexistent file. Confirm the user-facing message states the
   cause and a recovery path.
7. Open the Hub and confirm it visibly distinguishes working, waiting,
   blocked, failed, and completed work.
8. Exercise backup and restore only with a disposable test folder. Confirm the
   restored files and record any startup-mutated state separately.
9. If camera/audio/video permissions are available, test one request in each
   category. If permissions or hardware are unavailable, record that as an
   environment result rather than treating it as a source failure.

## Evidence to capture

For every step record: exact spoken/typed request, visible response, elapsed
time, output path, screenshots or screen recording, relevant Atlas log path,
and whether the result was pass, fail, or unavailable. Do not claim owner
acceptance from source tests or HTTP contracts alone.

## Repair rules

- Reproduce a failure twice before changing code.
- Keep changes inside Foundation Recovery: startup, conversation/context,
  background jobs, restart/recovery, Hub truthfulness, files/backup, and safety
  boundaries.
- Do not expand Creative Workspace or add integrations during this pass.
- Rebuild the installed executable after any source change and rerun the
  installed verifier.
- Preserve the rollback executable until the repaired candidate passes.

## Current known boundary

The Codex desktop-control bridge was unavailable during this handoff: it
exposed no native application surfaces. Therefore the visual UI and human-use
acceptance steps above remain unexecuted here. The technical installed runtime
is green, but the Atlas charter must not be marked fully complete until these
desktop steps are exercised and accepted.
