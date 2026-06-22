# sixpack Documentation Map

This page maps the public documentation, active design notes, implementation
status pages, and archived reference material for sixpack.

## Active Project Docs

- [README.md](../../README.md) — public overview, API shape, benchmarks, and repository layout.
- [book/README.md](../../book/README.md) — design book for philosophy, specs, and implementation direction.
- [book/13-sqlite-mapping.md](../../book/13-sqlite-mapping.md) — canonical SQLite-to-sixpack `get`/`watch`/`write` mapping.
- [book/14-write-engine.md](../../book/14-write-engine.md) — canonical batch-first write engine outline.
- [AGENTS.md](../../AGENTS.md) — working constraints for this repository and the source-of-truth model for implementation.
- [packages/docs/commands.md](commands.md) — CLI contract (currently `--version` and `help`).
- [packages/docs/file-format.md](file-format.md) — `.6`, `.6b`, metadata, and local directory layout.
- [tests/contracts/README.md](../../tests/contracts/README.md) — contract test boundary intent.
- [tests/snapshots/README.md](../../tests/snapshots/README.md) — snapshot testing intent.
- [benchmark/README.md](../../benchmark/README.md) — benchmark intent.
- [apps/landing-page/index.html](../../apps/landing-page/index.html) — static docs app for the current backend map and storage layout.
- [apps/admin-ui/README.md](../../apps/admin-ui/README.md) — admin UI intent.
- [apps/test-lab/README.md](../../apps/test-lab/README.md) — isolated experiments for generated examples, speed checks, fixtures, and UI prototypes.
- [packages/sixpack-testkit/src/lib.rs](../sixpack-testkit/src/lib.rs) — shared test helper crate.
- [packages/sixpack-schema-compiler/src/lib.rs](../sixpack-schema-compiler/src/lib.rs) — build-time schema parser/validator/output.

## Archived Reference Material

- [book/reference](../../book/reference/README.md) — older drafts and long-form background specs.

## Source-of-Truth Notes

- `book/` is the active design source of truth.
- `book/reference/` is background reference material and may contain older examples.

## Status Pointers

- Implemented status today is tracked in [book/07-implementation-status.md](../../book/07-implementation-status.md).
- Target generated API direction is tracked in [book/03-generated-api.md](../../book/03-generated-api.md).
- Simple SQLite operation equivalents are tracked in [book/13-sqlite-mapping.md](../../book/13-sqlite-mapping.md).
- Batch-first storage mutation direction is tracked in [book/14-write-engine.md](../../book/14-write-engine.md).
- Internal operation envelope direction is tracked in [book/04-plan-envelope.md](../../book/04-plan-envelope.md).
