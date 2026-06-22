# AGENTS.md

This file is the implementation contract for automated coding agents working in
this repository. It defines the authoritative project boundaries, current
runtime status, and verification rules agents must follow.

## Project Purpose

`sixpack` is a local-first database layer for small tools, agent runtimes,
desktop apps, research projects, and quantitative workflows. The root workspace
is a **v0 scaffold**: crate boundaries exist, and a minimal runtime path is
implemented for typed schema validation, `.6` append writes, generated `.6b`
indexes, and simple `get` / `write` APIs.

## Target Package Boundaries

The repository should continue toward these stable boundaries:

- `packages/sixpack-core`: shared domain types (workspace identity, model boundaries).
- `packages/sixpack-format`: durable file format boundary (header/validation/parsing paths).
- `packages/sixpack-store`: local storage engine boundary.
- `packages/sixpack`: composed runtime API (the DB handle + public orchestration).
- `packages/sixpack-cli`: CLI parsing/execution layer.
- `packages/sixpack-schema-compiler`: schema compilation crate (`schema!` parsing, validation, and raw Rust output).
- `apps/sixpack`: runnable binary that wires startup and delegates to `sixpack-cli`.
- `apps/admin-ui` (planned): local viewer/admin surface.
- `apps/test-lab` (experimental): broader test environment for temporary experiments, fixtures, and benchmark checks.
- `tests/contracts`: executable behavior contracts over public behavior.
- `tests/snapshots`: reviewed, stable-output regression assets.
- `packages/docs/file-format.md` and `packages/docs/commands.md`: public-facing user docs.
- `book/13-sqlite-mapping.md`: canonical mapping from simple SQLite operations
  to generated sixpack `get` / `watch` / `write` syntax.
- `book/14-write-engine.md`: canonical outline for the batch-first `.6` write
  engine and recoverable metadata rules.
- `user-scripts/install.sh`: local install script once the shell is feature-complete.

## Current Implementation Status

Current code includes:

- App entrypoint is `apps/sixpack`.
- CLI surface currently documents only:
  - `sixpack --version`
  - `sixpack help`
- Core behavior includes:
  - minimal schema primitives in `packages/sixpack-core`,
  - legacy JSONL event encoding/decoding helpers in `packages/sixpack-format`,
  - append-only `.6` write batches, recoverable `sixpack.toml` counters, and generated `.6b` lookup caches in `packages/sixpack-store`,
  - a composed `get` / `write` / `write_many` API in `packages/sixpack`.
- Schema compiler in `packages/sixpack-schema-compiler` with:
  - `schema!` parser for importable schema snippets,
  - compile-time validation for naming/lookups/duplicates,
  - optional raw Rust row/table emission for generated APIs.

## Temporary / Non-Authoritative Material

- Any file that says “prototype,” “draft,” “placeholder,” “plan,” or “temporary”
  must be treated as not shipped logic.

Use these as references only; do not use them as the source of truth for what is
currently implemented.

## Canonical References

- `book/README.md` and `book/SUMMARY.md` for internal design philosophy and the
  current build direction.
- focused chapters in `book/` for schema, API, plan envelope, storage, file
  format, status, package boundaries, naming, testing, and schema compiler work.
- `README.md` and `packages/docs/project-specs.md` for public structure and doc map.
- `book/13-sqlite-mapping.md` for the authoritative explanation of how simple
  SQLite-shaped operations map to sixpack's schema-declared selectors and changes.
- `book/14-write-engine.md` for the authoritative write-path outline. If code
  and docs drift, restore the batch-first invariant described there.
- `packages/docs/commands.md`, `packages/docs/file-format.md`,
  and `book/11-testing.md` for supporting contract language and test strategy.

Older long-form docs such as `sixpack_rust_backend_architecture.md` and
`sixpack_functional_addendum.md` are archived under `book/reference/`. If they
conflict with the book chapters, the book wins.

## Core Constraints

- Keep storage local to process directory-backed data; do not introduce hosted DB
  dependencies as the primary engine (SQL databases included).
- Do not add SQL or a generic query-string grammar as the normal product API;
  simple SQLite-shaped operations should map to generated selectors consumed by
  `db.get(...)`, generated changes consumed by `db.write(...)`, and future
  subscriptions through `db.watch(...)`.
- Do not expose storage internals as part of normal user APIs.
- Keep `.6` chunk files flat under each table directory
  (`tables/<table>/<chunk>.6`). Do not reintroduce generation folders unless
  explicitly requested.
- Do not claim “implemented” when a feature is only planned or stubbed.
- Avoid speculative abstractions outside the existing boundary model.
- Keep crate responsibilities aligned with the boundary list above.
- CLI is currently a small command surface. Do not imply richer CLI behavior
  until code and tests exist.
- Keep one interactive behavior path out of scope until it is explicitly needed.
- If a richer interactive command mode is added later, use terminal primitives via
  a maintained library like Ratatui for that narrow scope.

## Edit Map

- `apps/sixpack` — runnable CLI app.
- `packages/sixpack-core` — domain types.
- `packages/sixpack-format` — file format behavior.
- `packages/sixpack-store` — storage engine behavior.
- `packages/sixpack` — public DB API composition.
- `packages/sixpack-cli` — CLI command behavior.
- `packages/sixpack-testkit` — shared test helpers.
- `packages/sixpack-schema-compiler` — schema parse/validate/codegen crate.
- `tests/contracts` — contract tests.
- `tests/snapshots` — reviewed snapshots.
- `apps/test-lab` — broad experiment workspace (UI prototypes + fixtures + speed/sync checks), separate from shipped admin UI.
- `benchmark` — benchmark definitions.
- `packages/docs` — current and archive documentation.
- `user-scripts` — install script location.

## Experiment Workflow

- Keep `apps/test-lab` for temporary experiments and ad-hoc checks.
- Keep experimental artifacts in one of:
  - `apps/test-lab/fixtures/*` (inputs)
  - `apps/test-lab/experiments/*` (active notes)
- On completion of an experiment, archive a short, decision-focused summary
  outside the public repository and remove noisy scratch notes from active paths.

## Testing Policy

- Use temporary directories for data-bearing tests.
- Never write disposable data to `.sixpack`, `.data`, or repository root paths.
- Contract tests should validate external behavior, not private internals.
- Snapshot tests should remain intentionally stable and manually reviewed.

```txt
temp dir -> open database -> perform action -> assert -> temp dir auto-cleans
```

## Required check commands (run from repo root)

```sh
cargo fmt --all
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

Equivalent `just` flow is optional:

```sh
just fmt
just check
just test
just lint
```

## Style

- Make edits minimal and local to the requested change.
- Prefer explicit naming, small modules, and boundary-oriented code.
- Write docs/comments only where behavior is non-obvious.
