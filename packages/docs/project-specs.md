# Tensack specs and implementation references

This is the consolidated map of public design/specification documents and how they relate to the current root workspace.

## Current root workspace (active)

- [README.md](../../README.md) — current scope, package layout, and build assumptions.
- [book/README.md](../../book/README.md) — internal design book for philosophy, specs, and current direction.
- [book/13-sqlite-mapping.md](../../book/13-sqlite-mapping.md) — canonical SQLite-to-Tensack `get`/`watch`/`write` mapping.
- [AGENTS.md](../../AGENTS.md) — working constraints for this repository and the source-of-truth model for implementation.
- [packages/docs/commands.md](commands.md) — CLI contract stub (currently only `--version`, `help`).
- [packages/docs/file-format.md](file-format.md) — file format scope stub.
- [tests/contracts/README.md](../../tests/contracts/README.md) — contract test boundary intent.
- [tests/snapshots/README.md](../../tests/snapshots/README.md) — snapshot testing intent.
- [benchmark/README.md](../../benchmark/README.md) — benchmark intent.
- [apps/landing-page/index.html](../../apps/landing-page/index.html) — static docs app for the current backend map and storage layout.
- [apps/admin-ui/README.md](../../apps/admin-ui/README.md) — admin UI intent.
- [apps/test-lab/README.md](../../apps/test-lab/README.md) — experimental test workspace for speed/sync checks, fixtures, and UI experiments.
- [packages/tensack-testkit/src/lib.rs](../tensack-testkit/src/lib.rs) — shared test helper placeholder in Rust.
- [packages/tensack-schema-compiler/src/lib.rs](../tensack-schema-compiler/src/lib.rs) — build-time schema parser/validator/output.

## Design and architecture specs

- [book/reference](../../book/reference/README.md) — older root-level drafts and long-form background specs.

## Duplicate / overlap notes

- `book/` is the current internal design source of truth.
- `book/reference/` is background reference material and may contain older examples.

## Direct comparison: current vs spec

- Implemented status today is tracked in [book/07-implementation-status.md](../../book/07-implementation-status.md).
- Target generated API direction is tracked in [book/03-generated-api.md](../../book/03-generated-api.md).
- Simple SQLite operation equivalents are tracked in [book/13-sqlite-mapping.md](../../book/13-sqlite-mapping.md).
- Internal operation envelope direction is tracked in [book/04-plan-envelope.md](../../book/04-plan-envelope.md).
