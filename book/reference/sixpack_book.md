# sixpack Book

This is the compact source of truth for what sixpack is becoming.

The expanded internal design book now lives in [book/README.md](book/README.md).

sixpack is a local-first database layer for small applications and tools. A
database is a directory on disk. Logical data is described by a schema, written
to readable `.6` chunks, and accelerated by rebuildable generated caches.

## Core Decisions

- No hosted database is the primary engine.
- No SQL-shaped public language.
- The normal user API is generated from schema.
- Storage internals are not the normal user API.
- `.6` chunks are canonical source data.
- `.6b` and `.6x` are generated state.
- Schema compilation is build-time work, not a runtime dependency.
- The CLI stays small until the runtime contracts are stable.

## Main Specs

Read these in order:

1. [sixpack_schema_spec.md](sixpack_schema_spec.md) - schema authoring, validation, primitive types, and generated table metadata.
2. [sixpack_api_spec.md](sixpack_api_spec.md) - target generated API: `insert`, `upsert`, `patch`, `remove`, `get`, `find`, `scan`, `count`.
3. [sixpack_plan_spec.md](sixpack_plan_spec.md) - internal operation envelope shared by runtime, CLI, admin UI, and future SDK surfaces.
4. [sixpack_storage_spec.md](sixpack_storage_spec.md) - local directory layout, `.6` chunks, `.6b` caches, and chunk naming.
5. [sixpack_implementation_status.md](sixpack_implementation_status.md) - what exists now versus what is still planned.

## Current Workspace Map

- `packages/sixpack-core` - schema, record, value, and workspace domain types.
- `packages/sixpack-format` - `.6` and `.6b` encoding/decoding boundary.
- `packages/sixpack-store` - local directory-backed storage engine.
- `packages/sixpack` - composed Rust runtime API.
- `packages/sixpack-cli` - CLI parsing and command execution.
- `packages/sixpack-schema-compiler` - `schema!` parser, validator, and Rust output.
- `apps/sixpack` - runnable CLI binary.
- `apps/landing-page` - static docs page for the current backend map.
- `apps/admin-ui` - planned local viewer/admin surface.
- `apps/test-lab` - experiments, fixtures, and non-shipped checks.

## Intended Runtime Flow

```txt
schema.sixpack
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
  [sixpack_implementation_status.md](sixpack_implementation_status.md) says it is.
- Use older long-form docs as background only.

## Background References

- [sixpack_rust_backend_architecture.md](sixpack_rust_backend_architecture.md)
- [sixpack_functional_addendum.md](sixpack_functional_addendum.md)
- [sixpack_chunk_naming_spec.md](sixpack_chunk_naming_spec.md)
- [sixpack_6_format_spec_v0_1.md](sixpack_6_format_spec_v0_1.md)
