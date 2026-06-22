# SQLite Mapping

sixpack should be easy to explain to someone who knows SQLite, but it should
not become SQL-shaped.

SQLite's normal surface is a string language:

```sql
SELECT * FROM messages WHERE conversation_id = ?;
```

sixpack's normal surface is generated selectors and changes:

```rust
db.get(messages::by::conversation_id(conversation_id))?;
```

The product rule is:

- no user-authored SQL strings
- no generic query-string grammar
- normal current-state access goes through `db.get(selector)`
- future live state goes through `db.watch(selector)`
- changes go through `db.write(change)`
- same-table change batches go through `db.write_many(changes)`
- generated selectors and changes build internal plans

## Mental Model

SQLite:

```txt
table schema -> SQL string -> SQL parser/planner -> storage engine
```

sixpack:

```txt
schema.sixpack
  -> schema compiler
  -> generated selectors and changes
  -> db.get(...) / db.write(...) / db.write_many(...)
  -> internal plan envelope
  -> local store
```

The selector or change is the user-facing equivalent of the simple SQL
statement. The plan envelope is the execution contract underneath it, not the
syntax users should normally write.

## Common Operation Mapping

### Create One Row

SQLite:

```sql
INSERT INTO messages (id, conversation_id, body, created_at)
VALUES (?, ?, ?, ?);
```

sixpack:

```rust
db.write(messages::add(row))?;
```

### Create Or Replace One Row

SQLite:

```sql
INSERT INTO messages (...)
VALUES (...)
ON CONFLICT(id) DO UPDATE SET ...;
```

sixpack:

```rust
db.write(messages::set(row))?;
```

### Get One Row By Id

SQLite:

```sql
SELECT * FROM messages WHERE id = ?;
```

sixpack:

```rust
db.get(messages::by::id(message_id))?;
```

Every table has an implicit unique `id` selector.

### Get One Row By Unique Lookup

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

sixpack:

```rust
db.get(users::by::email(email))?;
```

Unique lookups generate selectors that return zero or one row.

### Get Many Rows By Lookup

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

sixpack:

```rust
db.get(messages::by::conversation_id(conversation_id))?;
```

Non-unique lookups generate selectors that return many rows.

### Edit One Row

SQLite:

```sql
UPDATE messages
SET body = ?
WHERE id = ?;
```

sixpack:

```rust
db.write(messages::edit(messages::key::id(message_id), patch))?;
```

Edits target one row through a unique key. For v1, edits cannot change `id`;
internally the runtime writes a full replacement row.

### Remove One Row

SQLite:

```sql
DELETE FROM messages WHERE id = ?;
```

sixpack:

```rust
db.write(messages::remove(messages::key::id(message_id)))?;
```

Removes resolve a unique target and write a tombstone internally.

### Apply Several Same-Table Changes

SQLite:

```sql
BEGIN;
UPDATE messages SET body = ? WHERE id = ?;
UPDATE messages SET body = ? WHERE id = ?;
COMMIT;
```

sixpack:

```rust
db.write_many([
    messages::edit(messages::key::id(first_id), first_patch),
    messages::edit(messages::key::id(second_id), second_patch),
])?;
```

`write_many` is not a general transaction language. It is the simple public
batch shape for one-table changes that can be validated before appending to the
current `.6` segment.

### Get A Page

SQLite:

```sql
SELECT * FROM messages LIMIT 100;
```

sixpack:

```rust
db.get(messages::all().limit(100))?;
```

`all` is allowed, but it is not the preferred replacement for every `WHERE`
clause. If application code commonly gets state by a field, that field should
usually be declared as a lookup.

### Get A Count

SQLite:

```sql
SELECT count(*) FROM messages;
```

sixpack:

```rust
db.get(messages::count())?;
```

## What a WHERE Clause Means in sixpack

Simple SQLite filters should map to schema decisions:

```sql
WHERE id = ?
WHERE email = ?
WHERE conversation_id = ?
```

In sixpack, those fields must be explicit lookups when they are normal access
paths:

```rust
lookup email unique
lookup conversation_id
```

That declaration is what allows generated selectors such as:

```rust
users::by::email(email)
messages::by::conversation_id(conversation_id)
```

Do not add a generic `where("field = value")` product API for v1. If an access
path matters, model it in the schema.

## Watch

`watch` should use the same selector values as `get`:

```rust
db.watch(messages::by::conversation_id(conversation_id), send_to_frontend)?;
```

That means subscriptions do not need their own query language. They subscribe to
the same declared current-state shape that `get` evaluates once.

`watch` is not implemented yet. Do not document it as shipped behavior until it
can actually keep subscribers updated after writes.

## Boundaries

The CLI and admin UI should eventually build the same internal plan envelope as
the generated API, but they should not imply that SQL is supported.

Current CLI docs remain intentionally narrow because the shipped CLI only
supports help and version commands.

The storage format remains separate from this syntax. `.6` files are durable
local data segments, `.6b` files are generated lookup caches, and neither is a
SQL database.
