# Tensack Plan Spec

The plan envelope is the internal operation contract.

Generated APIs, CLI commands, admin UI actions, and future SDKs should all map
to this shape before execution. The plan is not the primary user-facing API.

## Envelope

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

## Fields

### op

Required string.

Allowed target operations:

```txt
insert
upsert
patch
remove
get
find
scan
count
```

### table

Required string.

The schema table name.

### lookup

Optional string.

The lookup field used by `get`, `find`, `remove`, or `patch`.

Examples:

```txt
id
email
conversation_id
created_at
```

### key

Object containing the exact lookup key.

Examples:

```json
{ "id": "m1" }
```

```json
{ "conversation_id": "cv1" }
```

For a single-field lookup, this is still an object so the envelope can later
support compound keys without changing shape.

### value

Object containing row data or patch data.

For `insert` and `upsert`, `value` is the full row:

```json
{
  "id": "m1",
  "conversation_id": "cv1",
  "body": "hello"
}
```

For `patch`, `value` is partial:

```json
{ "body": "updated" }
```

For read operations, `value` is empty.

### range

Object for ordered or ranged lookup constraints.

Initial exact-key lookups do not need this.

Future examples:

```json
{ "gte": 1700000000, "lt": 1800000000 }
```

### limit

Optional positive integer.

Applies to `find` and `scan`. It may also be useful for future ranged reads.

### cursor

Optional opaque cursor.

The runtime owns cursor encoding. Users and SDKs should pass it through without
parsing it.

## Operation Semantics

### insert

```json
{
  "op": "insert",
  "table": "users",
  "value": {
    "id": "u1",
    "email": "a@test.com"
  }
}
```

Creates a row and fails if the id already exists.

### upsert

```json
{
  "op": "upsert",
  "table": "users",
  "value": {
    "id": "u1",
    "email": "new@test.com"
  }
}
```

Creates or fully replaces a row.

### patch

```json
{
  "op": "patch",
  "table": "users",
  "lookup": "id",
  "key": { "id": "u1" },
  "value": { "email": "new@test.com" }
}
```

Reads the current row, applies a partial change, and writes a full replacement
row internally.

### remove

```json
{
  "op": "remove",
  "table": "users",
  "lookup": "id",
  "key": { "id": "u1" }
}
```

Writes a tombstone.

### get

```json
{
  "op": "get",
  "table": "users",
  "lookup": "email",
  "key": { "email": "a@test.com" }
}
```

Requires a unique lookup and returns one row or none.

### find

```json
{
  "op": "find",
  "table": "messages",
  "lookup": "conversation_id",
  "key": { "conversation_id": "cv1" },
  "limit": 100,
  "cursor": null
}
```

Uses a lookup and returns many rows.

### scan

```json
{
  "op": "scan",
  "table": "messages",
  "limit": 100,
  "cursor": null
}
```

Returns live rows without a lookup key.

### count

```json
{
  "op": "count",
  "table": "messages",
  "lookup": "conversation_id",
  "key": { "conversation_id": "cv1" }
}
```

Returns a count of live matching rows.

## Validation Rules

- `table` must exist in schema.
- `lookup` must exist for `get` and `find`.
- `get`, `patch`, and `remove` require unique lookup targets.
- `insert` and `upsert` require a full row value.
- `patch` requires a non-empty partial value.
- Unknown fields in `value` are rejected.
- Values must match schema primitive types.
- `limit` must be bounded by runtime policy.

## Current Implementation

Implemented today:

- `PlanOp`
- `PlanEnvelope`
- `PlanOutcome`
- `TensackDatabase::execute_plan`
- `insert`
- `upsert`
- `patch`
- `remove`
- `get`
- `find`
- `scan`
- `count`

Current compatibility methods such as `insert`, `put`, `get`, `get_by`,
`get_many_by`, `patch_by_id`, `scan`, and `count` route through the plan layer
where practical.

Not implemented yet:

- JSON serialization/deserialization for the envelope.
- CLI commands that accept or emit plan envelopes.
- Admin UI execution through plan envelopes.
