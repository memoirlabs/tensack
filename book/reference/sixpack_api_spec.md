# sixpack API Spec

This spec defines the target public API shape.

The public API should be generated from schema. Users should work through table
handles and table-specific lookup names, not storage paths or generic stringly
typed commands.

## Target Shape

Conceptually:

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

Example:

```txt
db.messages.insert(row)
db.messages.upsert(row)
db.messages.patch({ id: "m1" }, { body: "updated" })
db.messages.remove({ id: "m1" })

db.messages.get.id("m1")
db.messages.get.slug("welcome")
db.messages.find.conversation_id("cv1")
db.messages.scan({ limit: 100 })
db.messages.count()
```

Rust may require method-call-friendly variants such as:

```rust
db.messages().insert(row)?;
db.messages().upsert(row)?;
db.messages().patch(messages::key::id("m1"), patch)?;
db.messages().remove(messages::key::id("m1"))?;

db.messages().get().id("m1")?;
db.messages().find().conversation_id("cv1")?;
db.messages().scan().limit(100).run()?;
db.messages().count()?;
```

or generated module functions:

```rust
messages::insert(&db, row)?;
messages::upsert(&db, row)?;
messages::get::id(&db, "m1")?;
messages::find::conversation_id(&db, "cv1")?;
```

The product decision is table-first and lookup-name-first. The exact Rust syntax
can adapt to what is clean and type-safe.

## Write Operations

### insert

Creates a new row.

- Requires a complete row.
- Fails if the row id already exists.
- Fails if a unique lookup key is already used by a different live row.
- Writes an append-only put row internally.

### upsert

Creates or fully replaces a row.

- Requires a complete row.
- Inserts if the id does not exist.
- Replaces the current live row if the id exists.
- Fails on unique lookup conflicts with other live rows.

Current runtime name: `put`.
Target public name: `upsert`.

### patch

Applies a partial update.

- Requires a unique row target.
- Accepts only changed fields.
- Produces a full replacement row internally after reading the current row.
- Fails if the target row does not exist.
- Fails on type errors or unique lookup conflicts.
- Current Rust implementation rejects `id` changes for v1 simplicity.

### remove

Deletes a row by unique target.

- Writes a tombstone.
- Does not require the full row body.
- Resolves arbitrary unique lookup targets through the plan layer, then deletes
  by id internally.

Current runtime name: `delete_by_id`.
Target public name: `remove`.

## Read Operations

### get

Reads one row through a unique lookup.

```txt
db.users.get.id("u1")
db.users.get.email("a@test.com")
```

Rules:

- Generated only for unique lookups.
- Returns one row or null/none.
- `id` is always available.

### find

Reads many rows through a lookup.

```txt
db.messages.find.conversation_id("cv1")
db.messages.find.created_at(...)
```

Rules:

- Generated for non-unique lookups.
- Can also be generated for unique lookups if a list-returning API is useful,
  but `get` remains the primary unique lookup API.
- Supports limit and cursor.

### scan

Reads rows from a table without requiring a lookup key.

Rules:

- Must support limit and cursor.
- Ordering must be explicit before it becomes a stable public guarantee.
- Current Rust implementation uses opaque offset cursors over live `.6b`
  entries; ordering is not yet a stable public guarantee.

### count

Counts rows matching a table, lookup, or scan plan.

Rules:

- Should use cache/index metadata when available.
- Must define whether it counts live rows only. The default is live rows only.
- Current Rust implementation counts live rows only.

## API To Plan Mapping

Generated public APIs should build internal plans described in
[sixpack_plan_spec.md](sixpack_plan_spec.md).

```txt
db.messages.find.conversation_id("cv1")
  -> { op: "find", table: "messages", lookup: "conversation_id", key: {...} }
```

The plan layer lets the CLI, admin UI, and generated SDKs share one execution
contract without exposing storage internals.

## Current Implementation

Implemented today:

- `Database::insert`
- `Database::put`
- `Database::delete_by_id`
- `Database::get`
- `Database::get_by`
- `Database::get_many_by`
- `Database::upsert`
- `Database::patch_by_id`
- `Database::scan`
- `Database::count`
- `Database::execute_plan`
- generated table handles from `emit_raw_rust`

Not implemented yet:

- stable generated API snapshots
