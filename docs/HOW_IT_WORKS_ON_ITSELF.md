# Atlas working on Atlas

The version before this went straight from a goal to an attempt. It would have
rotted, and the reason is worth writing down because it isn't obvious while
everything is green.

## Why green tests aren't enough

Two failures are invisible to a test suite:

**You fix the symptom.** The test passes, the cause is untouched, and it comes
back somewhere else in three weeks. A symptom patched somewhere other than the
cause passes *exactly as well* as a real fix.

**You write a test that would have passed anyway.** It proves nothing, it will
never fail again, and you'll trust it later. That's worse than no test.

## The rule that catches both

**Before writing any code: say what would prove it fixed, and check that the
proof fails right now.**

A proving test that already passes is testing something else. This is one line
of discipline and it is most of the value in the whole loop.

## The five stages

Each produces an artefact. You cannot enter a stage without the previous one's
artefact, and nothing lands without all five.

**Thought** — the symptom, the cause, *where* the cause is, the test that will
prove it, confirmation that test fails today, and what this deliberately isn't
fixing.

Rejected if: the cause is the symptom said again. "It's slow" / "because it's
slow" is the commonest version and it stops the work immediately.

**Build** — the change. Must make the proving test pass and break nothing.

**Review** — the check that does the work: **did the change happen where the
thought said the cause was?** If not, the test may be passing for a different
reason. Also catches a test edited in the same change as the fix it's meant to
prove, which is the oldest way to make a suite green and meaningless.

**Refine** — each refinement must name the review note it answers. One that
answers nothing is scope creep wearing a hat.

**Implement** — lands, with the change described as behaviour. No filenames, no
diffs, and it says what was left alone.

## Bounded

Three rounds of review and refine. A third attempt at the same problem usually
means the thought was wrong rather than the code, so it stops and hands over
rather than grinding.

## What it still can't do

It can't edit its own permissions — `policy.rs`, `finance.rs`, `consent.rs` and
`Cargo.toml` are off limits, and the test count is compared before and after.
That hasn't changed and shouldn't.
