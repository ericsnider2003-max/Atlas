# Applying the loop to everything built before it

> **STALE — the numbers describe a much smaller tree.** It says 145 modules. The five ratchets it describes are still real and have grown to twenty. Last true on early September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

Doing thought-build-review-refine by hand across 145 modules isn't work anyone
should do. It's work that should be **found** — so the pipeline's questions are
asked mechanically, of the whole codebase, every time the tests run.

Five checks, each a ratchet. The numbers only go down.

## What was found

**Every test in the suite asserts something.** The one hit was the audit
matching its own string literals. That was the check I most expected to fail
and it didn't.

**One module had behaviour and no test that called it** — `profiles`, which
keeps one person's assistant from knowing another person's business. That one
wants testing more than most, not less. Ten tests now, including the one that
matters: a name can't climb out of the profiles folder.

**Two modules had a label instead of an explanation** — `error` and `config`.
Both now say why they are the way they are, which is the part that stops
someone changing them for a good-sounding reason later.

**37 tests assert only that a constant contains a phrase.** Worth being honest
about: those prove the wording, not the behaviour. They're documentation with a
test harness around them. Useful — but they should never be the *only* test of
a behaviour, so the count is pinned and can't grow.

## The checks

| | catches |
|---|---|
| `no_test_asserts_nothing` | a test that passes for as long as the code compiles |
| `the_number_of_documentation_shaped_tests_does_not_grow` | wording tests quietly becoming the whole suite |
| `every_module_with_behaviour_is_exercised_somewhere` | code nothing calls, in a test or otherwise |
| `every_module_says_what_it_is_for` | a file whose documentation stopped matching it |
| `nothing_claiming_to_be_fixed_can_be_set_from_config` | a "not configurable" guarantee quietly becoming configurable |

That last one is the most important and it passes today: every place in the
codebase that claims something can't be turned off has a field that genuinely
can't be read from YAML.

## Why this rather than a review

A hand review of 145 modules is done once, is out of date the following week,
and its findings live in a document nobody opens. These run on every commit and
fail the build.

The honest limit: they catch structural failures, not wrong logic. A test can
assert something and still assert the wrong thing. That's what the review stage
of the pipeline is for, and it needs a person or a model — but it now only has
to look at the things these checks can't see.
