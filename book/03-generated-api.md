# Generated API

The generated API is the intended user-facing API.

It should hide storage details and avoid generic table/lookup strings in normal
application code.

For a direct comparison between common SQLite statements and the generated
Tensack shape, see [SQLite Mapping](13-sqlite-mapping.md).

## Operations

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

## Writes

### insert

Creates a new row.

- Requires a complete row.
- Fails if id already exists.
- Fails on unique lookup conflict.

### upsert

Creates or fully replaces a row.

- Requires a complete row.
- Inserts if id is missing.
- Replaces if id exists.
- Fails on unique lookup conflict with another row.

The internal/runtime compatibility name `put` can remain, but generated public
APIs should use `upsert`.

### patch

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

## Reads

### get

Reads one row through a unique lookup.

```txt
db.users.get.id("u1")
db.users.get.email("a@test.com")
```

### find

Reads many rows through a lookup.

```txt
db.messages.find.conversation_id("cv1")
```

### scan

Reads live rows from a table.

Must support limit and cursor. Ordering is not stable until explicitly defined.

### count

Counts live rows for a table or lookup.

## What Not To Do

Do not make the main generated API look like:

```rust
db.get_by("messages", "conversation_id", "cv1")
```

That can exist as compatibility/runtime glue, but it is not the product API.
