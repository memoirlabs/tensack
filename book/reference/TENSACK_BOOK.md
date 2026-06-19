# Tensack Book

This is the compact source of truth for what Tensack is becoming.

The expanded internal design book now lives in [book/README.md](book/README.md).

Tensack is a local-first database layer for small applications and tools. A
database is a directory on disk. Logical data is described by a schema, written
to readable `.ten` chunks, and accelerated by rebuildable generated caches.

## Core Decisions

- No hosted database is the primary engine.
- No SQL-shaped public language.
- The normal user API is generated from schema.
- Storage internals are not the normal user API.
- `.ten` chunks are canonical source data.
- `.tenb` and `.tenx` are generated state.
- Schema compilation is build-time work, not a runtime dependency.
- The CLI stays small until the runtime contracts are stable.

## Main Specs

Read these in order:

1. [TENSACK_SCHEMA_SPEC.md](TENSACK_SCHEMA_SPEC.md) - schema authoring, validation, primitive types, and generated table metadata.
2. [TENSACK_API_SPEC.md](TENSACK_API_SPEC.md) - target generated API: `insert`, `upsert`, `patch`, `remove`, `get`, `find`, `scan`, `count`.
3. [TENSACK_PLAN_SPEC.md](TENSACK_PLAN_SPEC.md) - internal operation envelope shared by runtime, CLI, admin UI, and future SDK surfaces.
4. [TENSACK_STORAGE_SPEC.md](TENSACK_STORAGE_SPEC.md) - local directory layout, `.ten` chunks, `.tenb` caches, and chunk naming.
5. [TENSACK_IMPLEMENTATION_STATUS.md](TENSACK_IMPLEMENTATION_STATUS.md) - what exists now versus what is still planned.

## Current Workspace Map

- `packages/tensack-core` - schema, record, value, and workspace domain types.
- `packages/tensack-format` - `.ten` and `.tenb` encoding/decoding boundary.
- `packages/tensack-store` - local directory-backed storage engine.
- `packages/tensack` - composed Rust runtime API.
- `packages/tensack-cli` - CLI parsing and command execution.
- `packages/tensack-schema-compiler` - `schema!` parser, validator, and Rust output.
- `apps/tensack` - runnable CLI binary.
- `apps/landing-page` - static docs page for the current backend map.
- `apps/admin-ui` - planned local viewer/admin surface.
- `apps/test-lab` - experiments, fixtures, and non-shipped checks.

## Intended Runtime Flow

```txt
schema.tensack
  -> schema compiler
  -> generated table API
  -> user calls db.<table>.<operation>()
  -> generated API creates a plan envelope
  -> runtime validates and executes the plan
  -> store writes/reads local files
  -> generated caches accelerate lookups
```

The public API should feel table-native:

```txt
db.<table>.insert()
db.<table>.upsert()
db.<table>.patch()
db.<table>.remove()

db.<table>.get.<unique_lookup>()
db.<table>.find.<lookup>()
db.<table>.scan()
db.<table>.count()
```

The engine should execute a compact internal plan:

```json
{
  "op": "find",
  "table": "messages",
  "lookup": "conversation_id",
  "key": { "conversation_id": "cv1" },
  "value": {},
  "range": {},
  "limit": 100,
  "cursor": null
}
```

## Source Of Truth Rules

- If this book conflicts with an older architecture draft, this book wins.
- If a focused spec conflicts with this book, update both in the same change.
- Do not claim an API or command is implemented until
  [TENSACK_IMPLEMENTATION_STATUS.md](TENSACK_IMPLEMENTATION_STATUS.md) says it is.
- Use older long-form docs as background only.

## Background References

- [tensack_rust_backend_architecture.md](tensack_rust_backend_architecture.md)
- [tensack_functional_addendum.md](tensack_functional_addendum.md)
- [tensack_chunk_naming_spec.md](tensack_chunk_naming_spec.md)
- [tensack_ten_format_spec_v0_1.md](tensack_ten_format_spec_v0_1.md)
