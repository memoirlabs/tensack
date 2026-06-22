# 0001 Generated Engine State Pack

## Status

Accepted direction, not fully implemented.

## Decision

sixpack remains a folder database. Canonical table data stays readable under
`tables/<table>/*.6`.

Generated binary engine state should move toward one private rebuildable pack:

```txt
engine/state.6pack
```

The pack may contain row pointers, lookup maps, counts, source hashes, and later
search-related generated state. It must be rebuildable from schema plus
canonical `.6` table data.

## Why

The product should feel like a database folder that a user can inspect without
being forced through a database shell. Table data stays obvious and readable.
Generated state is implementation detail, so it should be hidden behind one
engine-owned file instead of spread across many user-visible cache files.

## Current Implementation

The current runtime still uses one generated `.6b` cache per table:

```txt
engine/messages.6b
engine/users.6b
```

That is allowed during the transition. Docs and tests must continue to call out
which layout is implemented now and which layout is target direction.

## Consequences

- `.6` remains the durable data contract.
- `state.6pack` is not primary data.
- Removing `state.6pack` must trigger rebuild, not data loss.
- Store code should hide the cache layout behind an engine-state boundary.
- Public APIs should not expose paths inside `engine/`.
