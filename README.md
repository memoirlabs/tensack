# Tensack

Tensack is a local-first backend shell for experimenting with a compact data model, durable file format, local storage engine, command workflow, and local admin UI.

This repository is intentionally only a v0 scaffold right now. The crate boundaries are present so implementation can land in small, reviewable pieces.

## Workspace

The database is composed in `packages/tensack`. CLI behavior lives in `packages/tensack-cli`. The runnable Tensack app lives in `apps/tensack` and wires process startup to that CLI surface.

- `packages/tensack` - public database API that composes the lower-level packages.
- `packages/tensack-core` - core data model and shared domain types.
- `packages/tensack-format` - file format parser, encoder, decoder, and validation boundary.
- `packages/tensack-store` - local storage engine boundary.
- `packages/tensack-cli` - command-line parsing and command behavior.
- `packages/tensack-schema-compiler` - schema parser + validator + raw Rust IR/codegen output for `schema!` files.
- `packages/tensack-testkit` - shared test harnesses, builders, and assertions for workspace tests.
- `apps/tensack` - runnable Tensack app. Its current interface is a command-line binary named `tensack`.
- `apps/admin-ui` - placeholder for the local database viewer (future user-facing surface).
- `apps/test-lab` - broader experimental workspace for non-shipped tests, speed/sync checks, fixtures, and UI mockups.
- `benchmark` - benchmark definitions and implementation comparisons.
- `tests/contracts` - public behavior contracts for CLI, format, and storage boundaries.
- `tests/snapshots` - reviewed output snapshots for command and format surfaces.
- `docs` - user-facing format and command documentation.
- `user-scripts` - local installation scripts.
- [project spec/doc map](docs/project-specs.md) - public spec and implementation reference documents gathered in one place.

## Status

The workspace now includes a minimal writable data path: schema-validated rows
are encoded into per-table `.ten` row segments, with a root `tensack.toml`
physical layout map and generated `.tenb` caches for id and declared lookup
reads. `.tenx` is reserved for optional full-text search and is not required for
normal reads.

The current product target is intentionally plain: a tiny local table database
with CRUD, typed primitive fields, and rebuildable lookup caches. No SQL, no
chat-specific primary surface, no external database.

### Minimal CRUD example

```rust
use tensack::{
    DatabaseSchema, PrimitiveType, Record, TableSchema, TensackDatabase,
};

let mut schema = DatabaseSchema::new();
let mut table = TableSchema::new("messages");
table.add_field("id", PrimitiveType::Id).unwrap();
table.add_field("body", PrimitiveType::Text).unwrap();
schema.add_table(table).unwrap();

let db = TensackDatabase::open_local_with_schema("..", "chat", schema);
let row = Record::new("messages")
    .with_id("m1")
    .unwrap()
    .with_field("body", "hello")
    .unwrap();

db.insert(&row).unwrap();

let found = db.get("messages", "m1").unwrap();
assert!(found.is_some());

let replacement = Record::new("messages")
    .with_id("m1")
    .unwrap()
    .with_field("body", "updated")
    .unwrap();
db.put(&replacement).unwrap();
db.delete_by_id("messages", "m1").unwrap();
```
