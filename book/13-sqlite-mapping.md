# SQLite Mapping

Tensack should be easy to explain to someone who knows SQLite, but it should
not become SQL-shaped.

SQLite's normal surface is a string language:

```sql
SELECT * FROM messages WHERE conversation_id = ?;
```

Tensack's normal surface is schema-declared access plus generated table methods:

```rust
db.messages().find().conversation_id(conversation_id)?;
```

The product rule is:

- no user-authored SQL strings
- no generic query-string grammar
- normal reads use declared lookups
- generated APIs build internal plans
- the runtime validates and executes those plans

## Mental Model

SQLite:

```txt
table schema -> SQL string -> SQL parser/planner -> storage engine
```

Tensack:

```txt
schema.tensack
  -> schema compiler
  -> generated table API
  -> typed method call
  -> internal plan envelope
  -> local store
```

The generated method call is the user-facing equivalent of the simple SQL
statement. The plan envelope is the execution contract underneath it, not the
syntax users should normally write.

## Common Operation Mapping

### Insert

SQLite:

```sql
INSERT INTO messages (id, conversation_id, body, created_at)
VALUES (?, ?, ?, ?);
```

Tensack:

```rust
db.messages().insert(row)?;
```

The row shape comes from the generated schema API. The runtime still validates
that the row matches the schema and that unique lookups are not violated.

### Upsert

SQLite:

```sql
INSERT INTO messages (...)
VALUES (...)
ON CONFLICT(id) DO UPDATE SET ...;
```

Tensack:

```rust
db.messages().upsert(row)?;
```

`upsert` is the public generated API name for full-row replacement or insert.
The lower-level compatibility name `put` can remain inside runtime glue.

### Read One Row By Id

SQLite:

```sql
SELECT * FROM messages WHERE id = ?;
```

Tensack:

```rust
db.messages().get().id(message_id)?;
```

Every table has an implicit unique `id` lookup.

### Read One Row By Unique Lookup

Schema:

```rust
schema! {
  users {
    id id
    email text

    lookup email unique
  }
}
```

SQLite:

```sql
SELECT * FROM users WHERE email = ? LIMIT 1;
```

Tensack:

```rust
db.users().get().email(email)?;
```

Unique lookups generate `get` methods because at most one live row can match.

### Read Many Rows By Lookup

Schema:

```rust
schema! {
  messages {
    id id
    conversation_id id
    body text

    lookup conversation_id
  }
}
```

SQLite:

```sql
SELECT * FROM messages WHERE conversation_id = ?;
```

Tensack:

```rust
db.messages().find().conversation_id(conversation_id)?;
```

Non-unique lookups generate `find` methods because many live rows can match.

### Patch One Row

SQLite:

```sql
UPDATE messages
SET body = ?
WHERE id = ?;
```

Tensack:

```rust
db.messages().patch(messages::key::id(message_id), patch)?;
```

Patch targets must be unique. For v1, patches cannot change `id`; internally
the runtime writes a full replacement row.

### Remove One Row

SQLite:

```sql
DELETE FROM messages WHERE id = ?;
```

Tensack:

```rust
db.messages().remove(messages::key::id(message_id))?;
```

Deletes resolve a unique target and write a tombstone internally.

### Scan

SQLite:

```sql
SELECT * FROM messages LIMIT 100;
```

Tensack:

```rust
db.messages().scan().limit(100).run()?;
```

Scan is allowed, but it is not the preferred replacement for every `WHERE`
clause. If application code commonly reads by a field, that field should usually
be declared as a lookup.

### Count

SQLite:

```sql
SELECT count(*) FROM messages;
```

Tensack:

```rust
db.messages().count()?;
```

Count operates through the same generated table handle and plan executor.

## What a WHERE Clause Means in Tensack

Simple SQLite filters should map to schema decisions:

```sql
WHERE id = ?
WHERE email = ?
WHERE conversation_id = ?
```

In Tensack, those fields must be explicit lookups when they are normal read
paths:

```rust
lookup email unique
lookup conversation_id
```

That declaration is what allows the generated API to expose:

```rust
db.users().get().email(email)?;
db.messages().find().conversation_id(conversation_id)?;
```

Do not add a generic `where("field = value")` product API for v1. If a read path
matters, model it in the schema.

## Boundaries

The CLI and admin UI should eventually build the same internal plan envelope as
the generated API, but they should not imply that SQL is supported.

Current CLI docs remain intentionally narrow because the shipped CLI only
supports help and version commands.

The storage format remains separate from this syntax. `.ten` files are durable
local data segments, `.tenb` files are generated lookup caches, and neither is a
SQL database.
