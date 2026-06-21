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
- `apps/landing-page` - static documentation app for the current backend map and storage layout.
- `apps/admin-ui` - placeholder for the local database viewer (future user-facing surface).
- `apps/test-lab` - broader experimental workspace for non-shipped tests, speed/sync checks, fixtures, and UI mockups.
- `benchmark` - benchmark definitions and implementation comparisons.
- `tests/contracts` - public behavior contracts for CLI, format, and storage boundaries.
- `tests/snapshots` - reviewed output snapshots for command and format surfaces.
- `packages/docs` - user-facing format and command documentation.
- `user-scripts` - local installation scripts.
- [book](book/README.md) - internal design book for product decisions, philosophy, and backend direction.
- [project spec/doc map](packages/docs/project-specs.md) - supporting spec and implementation references.

## Canonical Design References

The book is the source of truth for current product direction. Start with:

- [Product shape](book/01-product-shape.md) - schema compiler, generated API, plan executor, and local store flow.
- [Generated API](book/03-generated-api.md) - intended `get` / `watch` / `write` user API.
- [Plan envelope](book/04-plan-envelope.md) - internal execution contract shared by generated APIs, CLI, and admin UI.
- [SQLite mapping](book/13-sqlite-mapping.md) - how simple SQLite operations map to Tensack syntax without adding SQL.

## Status

The workspace now includes a minimal writable data path: schema-validated rows
are encoded into per-table `.ten` row segments, with a root `tensack.toml`
physical layout map and generated binary `.tenb` caches for id lookups,
declared lookup reads, scans, and counts. `.tenx` is reserved for optional
full-text search and is not required for normal reads.

The current product target is intentionally plain: a tiny local table database
with `get` selectors, `write` changes, typed primitive fields, and rebuildable lookup caches. No SQL, no
chat-specific primary surface, no external database. Simple SQLite-shaped ideas
map to declared selectors and changes, not to a user-authored query-string
language.

The target public API is intentionally tiny:

```txt
db.get(selector)       current state once
db.watch(selector)     current state kept updated (planned)
db.write(change)       apply a declared change
```

Selectors and changes are generated from schema, for example
`messages::by::conversation_id("cv1")` or `messages::add(row)`.

Generated Rust table handles now build the internal plan envelope described in
[book/04-plan-envelope.md](book/04-plan-envelope.md). CLI commands and admin UI
actions should use that same executor as their surfaces expand. See
[book/13-sqlite-mapping.md](book/13-sqlite-mapping.md) for the intended mapping
from common SQLite operations to Tensack syntax.

## Current Benchmarks

Local Criterion run:

```sh
cargo bench -p tensack-benchmark --bench crud_vs_sqlite
```

Recent 100-row state-access results on this machine:

| Operation | Tensack | SQLite comparison |
| --- | ---: | ---: |
| write add one-by-one | ~40.4 ms | ~27.6 ms disk create |
| batched write add | ~1.08 ms | ~54 us in-memory transaction |
| get by id | ~3.26 ms | ~0.42 ms disk select |
| write edit by id | ~53.9 ms | ~19.1 ms disk update |
| write remove by id | ~47.9 ms | ~20.0 ms disk delete |

These are early engine benchmarks, not guarantees. They are useful for tracking
direction: batched appends are already much faster than one-row-at-a-time writes,
while get/edit/remove still have room to improve.

### Minimal State Example

```rust
use tensack::{
    DatabaseSchema, PrimitiveType, Record, TableSchema, TensackDatabase,
    change, selector,
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

db.write(change::add(row)).unwrap();

let found = db.get(selector::id("messages", "m1")).unwrap();
assert!(found.is_some());

let replacement = Record::new("messages")
    .with_id("m1")
    .unwrap()
    .with_field("body", "updated")
    .unwrap();
db.write(change::set(replacement)).unwrap();
db.write(change::remove_id("messages", "m1")).unwrap();
```
