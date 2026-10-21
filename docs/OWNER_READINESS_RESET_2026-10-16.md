# Owner readiness reset — Atlas

**Last updated:** 2026-10-20
**Status:** owner-raised concern; active reset plan  
**Owner signal:** Eric is not satisfied with the current status. After more than a month of work, Atlas still does not feel like an operating system with its foundation laid. The amount of code, design input, tests, and documentation has not translated into enough dependable day-to-day capability.

## The correction

Green tests, source completeness, design quality, and capability catalogs are evidence of engineering work. They are not by themselves evidence that Atlas is a usable operating system.

The primary success question is now:

> Can Eric use Atlas for ordinary work, with the core foundation present, without having to debug, configure, restart, interpret gaps, or sit through another design cycle?

Until the answer is yes and demonstrated, Atlas is not considered ready regardless of test count.

## What this means about the current status

- The repository has a substantial, tested foundation: typed capability evidence, durable jobs and receipts, restart reconciliation, Hub projections, safety boundaries, and offline/lean build coverage.
- The system is still not proven as a dependable installed operating system. Installed runtime, real device, real provider, real-folder, memory-pressure, and owner workflow evidence remain separate from the source gates.
- The project has accumulated too many parallel layers of implementation, design, evidence, and historical copies. That has made progress look larger than the user-visible result.
- The current gaps are not to be answered by another broad design pass. They must be reduced to observable operating behavior, one foundation slice at a time.

## Reset plan

### 1. Freeze expansion

Pause new integrations, content features, replica work, and broad capability additions. Keep only work that directly makes the operating foundation usable or proves a release blocker closed.

### 2. Define the minimum operating foundation

The foundation must provide, end to end:

1. Atlas starts and stays alive.
2. Atlas does not take over the computer's memory or responsiveness.
3. A normal typed or spoken request produces a visible, truthful result.
4. Background work starts, progresses, finishes, or reports a durable failure.
5. Restart does not lose work or silently replay uncertain work.
6. The Hub loads and shows what is working, blocked, waiting, or failed.
7. Files and state are saved safely, with backup and recovery demonstrated.
8. External or destructive actions remain behind clear approval boundaries.

### 3. Prove the foundation on the real laptop

The release candidate must be installed and exercised on the laptop. Source-only and synthetic tests remain useful but cannot close these gates:

- install the candidate build and record its version/hash;
- run the startup, conversation, Hub, background-job, restart, and recovery journeys;
- measure memory and responsiveness while Atlas is working;
- deliberately exercise a bounded failure and confirm the user sees the cause and recovery path;
- prove a real backup and restore;
- obtain owner acceptance after a short guided working session.

### 4. Convert every remaining gap into one of three outcomes

Each open item must become exactly one of:

- **Closed:** demonstrated in the installed system and accepted;
- **Blocked:** named external dependency or owner decision, with the smallest unblock step;
- **Removed from the current release:** explicitly deferred, with no claim that Atlas currently supports it.

“Designed,” “source-tested,” “wired,” or “likely works” are not release outcomes.

### 5. Report progress in user-visible terms

Future status updates should lead with:

- what Eric can do now;
- what still fails or is unproven in the installed system;
- the single next proof or decision;
- whether the result changed the readiness judgment.

Test counts and implementation details are supporting evidence only.

## Current readiness decision

**Decision: not ready to call Atlas an operating system.** The codebase is worth continuing, but the project must stop treating accumulated implementation as equivalent to a laid foundation. The next meaningful milestone is a small, installed, owner-accepted operating foundation—not another large feature batch.

## Success gate for leaving this reset

This reset is complete only when the owner can use the installed Atlas for a normal working session and say that it is helping rather than creating another debugging project. The evidence package must include the installed build identity, the core journey results, memory/response observations, recovery proof, and the explicit list of what remains deferred.
