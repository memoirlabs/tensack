# Plan Envelope

The plan envelope is the internal operation contract.

Generated APIs, CLI commands, admin UI actions, and future SDKs should converge
on this shape before execution.

The plan is not the primary user-facing API. Users should normally deal in
`db.get(selector)`, future `db.watch(selector)`, and `db.write(change)`.

## Shape

```json
{
  "op": "...",
  "table": "...",
  "lookup": "...",
  "key": {},
  "value": {},
  "range": {},
  "limit": 100,
  "cursor": null
}
```

## Operations

```txt
add
set
edit
remove
get
many
page
count
```

## Examples

Add:

```json
{
  "op": "add",
  "table": "users",
  "value": {
    "id": "u1",
    "email": "a@test.com"
  }
}
```

Many:

```json
{
  "op": "many",
  "table": "messages",
  "lookup": "conversation_id",
  "key": { "conversation_id": "cv1" },
  "limit": 100,
  "cursor": null
}
```

Edit:

```json
{
  "op": "edit",
  "table": "users",
  "lookup": "id",
  "key": { "id": "u1" },
  "value": { "email": "new@test.com" }
}
```

## Validation

- `table` must exist.
- `lookup` must exist where required.
- `get`, `edit`, and `remove` require unique lookup targets.
- `add` and `set` require full row values.
- `edit` requires a non-empty partial value.
- Unknown fields are rejected.
- Values must match schema primitive types.
- `limit` must be bounded.

## Implementation

Current runtime has:

```rust
GetRequest
WriteRequest
PlanOp
PlanEnvelope
PlanOutcome
Database::get
Database::write
Database::write_many
Database::execute_plan
```

`write_many` is not a different operation model. It resolves several
same-table write plans into one storage batch so the store can append one `.6`
chunk and publish one generated index snapshot.

Still needed:

- JSON serde for plan envelopes.
- CLI plan execution.
- Admin UI plan execution.
