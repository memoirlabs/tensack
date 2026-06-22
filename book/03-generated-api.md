# Generated API

The generated API is the intended user-facing API.

It should hide storage details and avoid generic table/lookup strings in normal
application code. The top-level product verbs are:

```txt
db.get(selector)
db.watch(selector)
db.write(change)
db.write_many(changes)
```

For a direct comparison between common SQLite statements and the generated
sixpack shape, see [SQLite Mapping](13-sqlite-mapping.md).

## Selectors

```txt
<table>::by::<unique_lookup>(value)  -> one row
<table>::by::<lookup>(value)         -> many rows
<table>::all().limit(n)              -> page of rows
<table>::count()                     -> row count
```

Examples:

```rust
db.get(users::by::id("u1"))?;
db.get(users::by::email("a@test.com"))?;
db.get(messages::by::conversation_id("cv1"))?;
db.get(messages::all().limit(100))?;
db.get(messages::count())?;
```

## Changes

```txt
<table>::add(row)                 -> create one row
<table>::set(row)                 -> create or replace one row
<table>::edit(target, patch)      -> partially change one row
<table>::remove(target)           -> remove one row
```

Examples:

```rust
db.write(messages::add(row))?;
db.write(messages::set(row))?;
db.write(messages::edit(messages::key::id("m1"), patch))?;
db.write(messages::remove(messages::key::id("m1")))?;
db.write_many([
    messages::edit(messages::key::id("m1"), first_patch),
    messages::edit(messages::key::id("m2"), second_patch),
])?;
```

## Semantics

### add

Creates a new row.

- Requires a complete row.
- Fails if id already exists.
- Fails on unique lookup conflict.

### set

Creates or fully replaces a row.

- Requires a complete row.
- Inserts if id is missing.
- Replaces if id exists.
- Fails on unique lookup conflict with another row.

### edit

Partially updates one row.

- Requires a unique target.
- Accepts only changed fields.
- Reads current row.
- Writes a full replacement row internally.
- Rejects `id` changes for v1 simplicity.

### remove

Deletes one row.

- Requires a unique target.
- Resolves target row.
- Writes a tombstone by id internally.

### get

Gets current state once.

- unique selectors return zero or one row
- lookup selectors return many rows
- `all` returns a page
- `count` returns a number

### watch

Watches current state as it changes. This is the planned subscription surface.
Do not claim `watch` is implemented until the runtime can actually keep
subscribers updated.

### write_many

Applies same-table changes as one storage batch.

- All changes must target the same table.
- Validation happens before disk writes.
- The store appends the batch into the current `.6` segment.
- This is implemented in the runtime as the fast path for grouped writes.

## What Not To Do

Do not add a second query language, string selector API, or table-command API as
the main product surface. Product code should pass generated values into
`db.get(...)`, future `db.watch(...)`, `db.write(...)`, and same-table
`db.write_many(...)`.
