# F1 durable job compile fix — 2026-10-15

## Result

The durable-job envelope now compiles through the task-loop integration.

## Defect

The task-loop start path combined a `Result<(), AtlasError>` store write with a
`Result<_, String>` transition, and the task-loop finish path still matched the
plain `Vec<Record>` returned by `job::load` as if it were a `Result`. The project
therefore failed at the library compile boundary even though the job module's
own tests were valid.

## Repair

- Convert the store-write error to the task-loop's existing string error lane.
- Treat `job::load` as the infallible store read it is, then persist the updated
  record through `job::save`.
- Keep the existing transition gate and warning behavior intact.

## Proof

- `cargo fmt --manifest-path atlas/Cargo.toml -- --check` — passed.
- `cargo check --manifest-path atlas/Cargo.toml --lib` — passed.
- Isolated target `target_record_verify`:
  `cargo test --manifest-path atlas/Cargo.toml --lib job::tests -- --test-threads=1`
  — 8 passed, 0 failed.

The full serial suite is not claimed here; concurrent Cargo work in this folder
required an isolated target for this focused proof.
