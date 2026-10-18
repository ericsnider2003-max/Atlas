# Session 2026-10-15: Foundation evidence and durable persistence paths are wired and locally verified; Hub status honors live research state, task-loop and routine persistence failures are observable, and focused gates remain green.

Last updated: 2026-10-15
By: agent

## Git at close

- main @ d3d5bca2e6; 95 uncommitted file(s); 0 ahead, 0 behind upstream; fetch: skipped

```
 M .gitattributes
 M .gitignore
 M atlas/docs/CAPABILITIES.md
 M atlas/src/apns.rs
 M atlas/src/astype.rs
 M atlas/src/capability.rs
 M atlas/src/connecting.rs
 M atlas/src/daemon.rs
 M atlas/src/daemon/away.rs
 M atlas/src/daemon/camera.rs
 M atlas/src/daemon/errands.rs
 M atlas/src/daemon/execute.rs
 M atlas/src/daemon/hands.rs
 M atlas/src/daemon/helping.rs
 M atlas/src/daemon/inbox.rs
 M atlas/src/daemon/late.rs
 M atlas/src/daemon/learning.rs
 M atlas/src/daemon/making.rs
 M atlas/src/daemon/messages.rs
 M atlas/src/daemon/model.rs
 M atlas/src/daemon/on_itself.rs
 M atlas/src/daemon/onethread.rs
 M atlas/src/daemon/reading.rs
 M atlas/src/daemon/running.rs
 M atlas/src/daemon/tasks.rs
 M atlas/src/daemon/tick.rs
 M atlas/src/daemon/turn/around_the_turn.rs
 M atlas/src/daemon/turn/run_command.rs
 M atlas/src/daemon/turn/unwatched.rs
 M atlas/src/feedback.rs
```

## Summary

Foundation evidence and durable persistence paths are wired and locally verified; Hub status honors live research state, task-loop and routine persistence failures are observable, and focused gates remain green.

## Next

```
cargo test --manifest-path atlas/Cargo.toml --test all -- capability_wiring -- --test-threads=1
```

## Verified

- Capability evidence reaches catalog, voice, and Hub status — focused capability honesty/routine/wiring tests pass
- Durable task-loop and routine persistence failures retain safe retry/defer behavior and now log errors — focused tests plus cargo check pass

## Unverified

- UNVERIFIED: Real-device, owner-selected folder, and external dependency measurements remain outside this workspace

## Blockers

- none recorded
