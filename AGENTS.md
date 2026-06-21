# AGENTS.md

This file is the behavior contract for future coding agents.

## What this repository is for

`tensack` is building a local-first data layer for a small database shell.
The root workspace is a **v0 scaffold**: crate boundaries exist, and a minimal
runtime path is implemented for typed schema validation + `.ten` append writes.

## Final target (authoritative)

The direction is to end up with:

- `packages/tensack-core`: shared domain types (workspace identity, model boundaries).
- `packages/tensack-format`: durable file format boundary (header/validation/parsing paths).
- `packages/tensack-store`: local storage engine boundary.
- `packages/tensack`: composed runtime API (the DB handle + public orchestration).
- `packages/tensack-cli`: CLI parsing/execution layer.
- `packages/tensack-schema-compiler`: schema compilation crate (`schema!` parsing, validation, and raw Rust output).
- `apps/tensack`: runnable binary that wires startup and delegates to `tensack-cli`.
- `apps/admin-ui` (planned): local viewer/admin surface.
- `apps/test-lab` (experimental): broader test environment for temporary experiments, fixtures, and benchmark checks.
- `tests/contracts`: executable behavior contracts over public behavior.
- `tests/snapshots`: reviewed, stable-output regression assets.
- `packages/docs/file-format.md` and `packages/docs/commands.md`: public-facing user docs.
- `book/13-sqlite-mapping.md`: canonical mapping from simple SQLite operations
  to generated Tensack syntax.
- `user-scripts/install.sh`: local install script once the shell is feature-complete.

## Current implementation status (important)

Current code now includes:

- App entrypoint is `apps/tensack`.
- CLI surface currently documents only:
  - `tensack --version`
  - `tensack help`
- Core behavior includes:
  - minimal schema primitives in `packages/tensack-core`,
  - legacy JSONL event encoding/decoding helpers in `packages/tensack-format`,
  - append-only `.ten` writes, `tensack.toml`, and generated `.tenb` lookup caches in `packages/tensack-store`,
  - a composed write/read API in `packages/tensack`.
- Schema compiler in `packages/tensack-schema-compiler` with:
  - `schema!` parser for importable schema snippets,
  - compile-time validation for naming/lookups/duplicates,
  - optional raw Rust row/table emission for generated APIs.

## Temporary / non-authoritative material

- Any file that says “prototype,” “draft,” “placeholder,” “plan,” or “temporary”
  must be treated as not shipped logic.

Use these as references only; do not use them as the source of truth for what is
currently implemented.

## Canonical references for present scope

- `book/README.md` and `book/SUMMARY.md` for internal design philosophy and the
  current build direction.
- focused chapters in `book/` for schema, API, plan envelope, storage, file
  format, status, package boundaries, naming, testing, and schema compiler work.
- `README.md` and `packages/docs/project-specs.md` for current structure and doc map.
- `book/13-sqlite-mapping.md` for the authoritative explanation of how simple
  SQLite-shaped operations map to Tensack's schema-declared, generated API.
- `packages/docs/commands.md`, `packages/docs/file-format.md`,
  and `book/11-testing.md` for supporting contract language and test strategy.

Older long-form docs such as `tensack_rust_backend_architecture.md` and
`tensack_functional_addendum.md` are archived under `book/reference/`. If they
conflict with the book chapters, the book wins.

## Core constraints (do not violate without instruction)

- Keep storage local to process directory-backed data; do not introduce hosted DB
  dependencies as the primary engine (SQL databases included).
- Do not add SQL or a generic query-string grammar as the normal product API;
  simple SQLite-shaped operations should map to declared lookups and generated
  table methods.
- Do not expose storage internals as part of normal user APIs.
- Do not claim “implemented” when a feature is only planned or stubbed.
- Avoid speculative abstractions outside the existing boundary model.
- Keep crate responsibilities aligned with the boundary list above.
- CLI is primarily a simple command surface and the controlling interface for the
  repo right now.
- Keep one interactive behavior path out of scope until it is explicitly needed.
- If a richer interactive command mode is added later, use terminal primitives via
  a maintained library like Ratatui for that narrow scope.

## Workspace map for edits

- `apps/tensack` — runnable CLI app.
- `packages/tensack-core` — domain types.
- `packages/tensack-format` — file format behavior.
- `packages/tensack-store` — storage engine behavior.
- `packages/tensack` — public DB API composition.
- `packages/tensack-cli` — CLI command behavior.
- `packages/tensack-testkit` — shared test helpers.
- `packages/tensack-schema-compiler` — schema parse/validate/codegen crate.
- `tests/contracts` — contract tests.
- `tests/snapshots` — reviewed snapshots.
- `apps/test-lab` — broad experiment workspace (UI prototypes + fixtures + speed/sync checks), separate from shipped admin UI.
- `benchmark` — benchmark definitions.
- `packages/docs` — current and archive documentation.
- `user-scripts` — install script location.

## Experiment workflow (non-hot-path)

- Keep `apps/test-lab` for temporary experiments and ad-hoc checks.
- Keep experimental artifacts in one of:
  - `apps/test-lab/fixtures/*` (inputs)
  - `apps/test-lab/experiments/*` (active notes)
- On completion of an experiment, archive a short, decision-focused summary
  outside the public repository and remove noisy scratch notes from active paths.

## Testing policy

- Use temporary directories for data-bearing tests.
- Never write disposable data to `.tensack`, `.data`, or repository root paths.
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
