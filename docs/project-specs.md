# Tensack specs and implementation references

This is the consolidated map of public design/specification documents and how they relate to the current root workspace.

## Current root workspace (active)

- [README.md](../README.md) — current scope, package layout, and build assumptions.
- [AGENTS.md](../AGENTS.md) — working constraints for this repository and the source-of-truth model for implementation.
- [SCHEMA_COMPILER.md](../SCHEMA_COMPILER.md) — current schema compiler behavior and integration notes.
- [docs/commands.md](commands.md) — CLI contract stub (currently only `--version`, `help`).
- [docs/file-format.md](file-format.md) — file format scope stub.
- [DATABASE_TESTING.md](../DATABASE_TESTING.md) — testing model and isolation rules.
- [tests/contracts/README.md](../tests/contracts/README.md) — contract test boundary intent.
- [tests/snapshots/README.md](../tests/snapshots/README.md) — snapshot testing intent.
- [benchmark/README.md](../benchmark/README.md) — benchmark intent.
- [apps/admin-ui/README.md](../apps/admin-ui/README.md) — admin UI intent.
- [apps/test-lab/README.md](../apps/test-lab/README.md) — experimental test workspace for speed/sync checks, fixtures, and UI experiments.
- [packages/tensack-testkit/src/lib.rs](../packages/tensack-testkit/src/lib.rs) — shared test helper placeholder in Rust.
- [packages/tensack-schema-compiler/src/lib.rs](../packages/tensack-schema-compiler/src/lib.rs) — build-time schema parser/validator/output.

## Design and architecture specs

- [tensack_rust_backend_architecture.md](../tensack_rust_backend_architecture.md) — full architecture/spec for readable `.ten` row segments, rebuildable lookup indexes, `tensack.toml` metadata, and generated registry direction.
- [tensack_functional_addendum.md](../tensack_functional_addendum.md) — functional interface addendum to the architecture.

## Duplicate / overlap notes

- `tensack_rust_backend_architecture.md` and `tensack_functional_addendum.md` are both active spec documents; the addendum is explicitly supplemental and should be read with the architecture document.
- Current root workspace currently implements only the CLI shell; it is far behind most of the behavior described in the architecture docs.

## Direct comparison: current vs spec

- Implemented status today: command-surface shell (`--version`, `help`) and package-level boundaries; a schema compiler crate is now present, while registry generation and full runtime behavior are still pending.
- Most spec items currently not present: full generated registries, binary-packed `.tenb` layout, `.tenx` search, read/change plans, compaction, repair, inspect, and SDK surface.
- The root package layout and boundaries are aligned with the architecture direction but not yet functionally complete.
