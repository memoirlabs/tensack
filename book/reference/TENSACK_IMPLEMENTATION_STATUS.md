# Tensack Implementation Status

This document separates shipped behavior from target design.

## Recent History Summary

- `c448837` (`ADD LATER`): added the note-taking init test-lab crate,
  schema-compiler build path, reverse chunk naming spec, and early generated
  SDK experiment.
- Current working change: adds the internal plan executor, generated table
  handles, `patch`/`scan`/`count`, remove-by-unique plan behavior, and
  binary-packed `.tenb` v2 caches.

## Implemented Now

### Workspace

- Rust workspace with package boundaries.
- App entrypoint at `apps/tensack`.
- Static docs landing page at `apps/landing-page`.

### CLI

- `tensack --version`
- `tensack -V`
- `tensack help`
- `tensack -h`
- `tensack --help`

### Core

- `DatabaseSchema`
- `TableSchema`
- `FieldSpec`
- `LookupSpec`
- `Record`
- `Value`
- primitive types: `id`, `text`, `int`, `float`, `bool`
- schema validation for required fields, unknown fields, and type mismatch

### Format

- `.ten` table preamble encoding
- `.ten` row operation encoding and decoding
- put rows and delete tombstones
- binary-packed `.tenb` v2 cache encoding and decoding
- legacy text `.tenb` decoding for rebuild migration
- source hashing for cache freshness

### Store

- local database directory layout
- per-table reverse-sorted chunk files
- `tensack.toml` metadata
- append insert
- append put/full replacement
- delete tombstone by id
- generated `.tenb` rebuilds
- id lookup
- declared lookup read
- table scan over live cache rows
- table and lookup counts
- unique lookup conflict checks

### Runtime API

Current compatibility string-based API:

```rust
db.init()?;
db.insert(&record)?;
db.upsert(&record)?;
db.put(&record)?;
db.patch_by_id("messages", "m1", patch)?;
db.delete_by_id("messages", "m1")?;
db.get("messages", "m1")?;
db.get_by("users", "email", "a@test.com")?;
db.get_many_by("messages", "conversation_id", "cv1")?;
db.scan("messages", Some(100), None)?;
db.count("messages")?;
db.rebuild_cache("messages")?;
```

Current internal plan API:

```rust
db.execute_plan(plan)?;
```

### Schema Compiler

- parses `schema! { ... }`
- validates duplicate tables and fields
- validates lookup targets
- validates primitive types
- emits raw Rust row/table handles, patches, lookup keys, scans, counts, and
  table extension traits

## Target But Not Implemented

- stable generated API snapshots
- admin UI
- inspect/repair CLI
- compaction
- `.tenx` full-text search

## Important Naming Decision

Current runtime name:

```txt
put
```

Target public generated API name:

```txt
upsert
```

The existing `put` behavior is the storage/runtime primitive for full row
replacement. User-facing generated APIs should prefer `upsert`.

## Required Checks

Run from repository root:

```sh
cargo fmt --all
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```
