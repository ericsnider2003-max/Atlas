# Install runbook, 2026-10-12 (UNCOMMITTED)

Build: branch integration/2026-10-11 @ c797d247, `cargo build --release --locked --bin atlas`
New exe: C:\Users\erics\Atlas\tgt-rel\release\atlas.exe  (59,072,000 bytes)
SHA-256: ac4692b84de74220ce38dd12d8c3f3c7accde81a29135af3e74b97c635cf9a11

Layout: install root C:\Users\erics\AppData\Local\Atlas (atlas.exe, config\, data\, models\, tools\, tor\).
Update = replace atlas.exe only; data\ and config\machine.yaml are untouched (docs/INSTALLING.md).
Atlas runs as scheduled task "Atlas" -> `atlas.exe --daemon` (triggers: logon, session change, event).
Port 8094 = sherpa-onnx-offline-websocket-server.exe (tools\sherpa\bin), a child/helper, not atlas.exe.

Do NOT run the new exe from tgt-rel to test: `atlas --version` there tries to start an install beside it.
Copy it into place first (step 3), or test via a copy in an empty temp folder.

## 1. Back up (PowerShell)
    $A="$env:LOCALAPPDATA\Atlas"; $B="C:\Users\erics\Atlas-backup-20261012"
    New-Item -ItemType Directory $B | Out-Null
    Copy-Item "$A\atlas.exe" "$B\atlas.exe"
    Copy-Item "$A\config" "$B\config" -Recurse
    robocopy "$A\data" "$B\data" /E /XD cache tmp webview /R:1 /W:1     # ~1.1 GB total data; cache/tmp/webview are rebuildable
    Get-FileHash "$A\atlas.exe","$B\atlas.exe"                          # hashes must match
models\ (large, downloaded) and tools\ , tor\ are not changed by an update; no need to copy.
Also keep the sibling binary: $A\atlas-0.1.0-dev.previous.exe already exists (older); do not rely on it.

## 2. Stop cleanly
Atlas.exe is NOT running at last check (only the sherpa helper, PID 16368). Re-check:
    Get-Process atlas* -EA 0
    Stop-ScheduledTask -TaskName Atlas          # stops daemon if the task is running it
    Get-Process atlas* -EA 0                    # expect nothing
If an atlas window is open, close it from its own UI (or the tray menu). Only if it will not exit, ask the owner before Stop-Process.
Sherpa on 8094 may stay; Atlas respawns/reuses it. If a restart of voice is wanted, stop it only after atlas.exe is gone.

## 3. Replace
    Rename-Item "$A\atlas.exe" "atlas.exe.before-20261012"
    Copy-Item C:\Users\erics\Atlas\tgt-rel\release\atlas.exe "$A\atlas.exe"
    Get-FileHash "$A\atlas.exe"     # expect ac4692b8...f9a11

## 4. Restart
    Start-ScheduledTask -TaskName Atlas         # same as logon start (atlas.exe --daemon)
(or Start menu "Atlas" shortcut for the window)

## 5. Verify
    & "$A\atlas.exe" --version     # expect "atlas 0.1.0-dev" and install: C:\Users\erics\AppData\Local\Atlas
    & "$A\atlas.exe" doctor
    Get-Process atlas*; Get-NetTCPConnection -LocalPort 8094 -State Listen
    Get-ScheduledTaskInfo -TaskName Atlas      # LastTaskResult 0
Note: --version prints 0.1.0-dev for every build, so confirm identity by SHA-256 above.

## 6. Rollback
    Stop-ScheduledTask -TaskName Atlas; Get-Process atlas* -EA 0   # none
    Remove-Item "$A\atlas.exe"; Rename-Item "$A\atlas.exe.before-20261012" atlas.exe
    Start-ScheduledTask -TaskName Atlas
If data/config misbehave (should not): restore config\ from $B\config; data from $B\data (robocopy $B\data $A\data /E) with Atlas stopped.
