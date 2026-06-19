# Plan Envelope

The plan envelope is the internal operation contract.

Generated APIs, CLI commands, admin UI actions, and future SDKs should converge
on this shape before execution.

The plan is not the primary user-facing API.

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
insert
upsert
patch
remove
get
find
scan
count
```

## Examples

Insert:

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

Find:

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

Patch:

```json
{
  "op": "patch",
  "table": "users",
  "lookup": "id",
  "key": { "id": "u1" },
  "value": { "email": "new@test.com" }
}
```

## Validation

- `table` must exist.
- `lookup` must exist where required.
- `get`, `patch`, and `remove` require unique lookup targets.
- `insert` and `upsert` require full row values.
- `patch` requires a non-empty partial value.
- Unknown fields are rejected.
- Values must match schema primitive types.
- `limit` must be bounded.

## Implementation

Current runtime has:

```rust
PlanOp
PlanEnvelope
PlanOutcome
TensackDatabase::execute_plan
```

Current compatibility APIs route through the plan layer where practical.

Still needed:

- JSON serde for plan envelopes.
- CLI plan execution.
- Admin UI plan execution.

