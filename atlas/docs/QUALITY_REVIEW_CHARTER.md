# Atlas Quality Review Charter

This is the review input set for the autonomous hardening loop. These projects
are review references and tools for analysis; they are not Atlas runtime
dependencies.

| Review input | Criterion applied to Atlas |
| --- | --- |
| [Hairline](https://github.com/lucasmarkes/hairline) | Keep changes minimal, explicit, and locally composable; reject unnecessary abstraction and coupling. |
| [Cloudflare security-audit-skill](https://github.com/cloudflare/security-audit-skill) | Perform independent security phases, record machine-checkable findings, and require evidence before closure. |
| [Unlazy](https://github.com/Leonxlnx/unlazy) | Decompose large tasks into independently verified leaves; do not accept shallow or premature green status. |
| [Karpathy llm-council](https://github.com/karpathy/llm-council) | Use independent reviewers, structured debate, disagreement capture, and adjudicated findings. |
| [Thermo-Nuclear Code Quality Review](https://github.com/cursor/plugins/tree/main/cursor-team-kit/skills/thermo-nuclear-code-quality-review) | Reject giant-file growth, tangled conditionals, weak boundaries, and maintainability regressions. |
| [CodeGraph](https://github.com/colbymchenry/codegraph) | Trace callers, dependants, and impact paths before changing a seam; verify wiring after edits. |

## Closure rule

A finding is not closed because a unit test passes. It requires a reproducer or
source-level proof, a bounded correction, a live or pressure test at the same
scope, and a second independent review. A remaining finding is classified as
unresolved until that evidence exists.
