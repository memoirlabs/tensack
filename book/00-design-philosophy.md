# Design Philosophy

Tensack should be small, local, inspectable, and schema-driven.

The main mistake to avoid is making Tensack feel like a generic database wrapper
or a pile of internal implementation details. The public surface should feel
like the user’s own data model.

## Principles

### Local first

A Tensack database is a directory on disk.

No hosted database is the primary engine. No SQL database is the primary engine.
SQLite, Postgres, and hosted services can be useful comparison targets or
future import/export integrations, but they are not the storage engine.

### Schema first

The user defines a schema. From that schema we generate:

- typed rows
- selectors
- changes
- validation metadata
- runtime plans

The normal path should not be generic stringly typed calls.

### Tiny API, simple engine

Users should call:

```txt
db.get(selector)
db.watch(selector)
db.write(change)
db.write_many(changes)
```

Generated schema code should provide selectors such as
`messages::by::conversation_id(cv)` and changes such as `messages::add(row)`.
Internally, those values can become a compact plan envelope. The engine should
execute plans, not expose storage internals.

### Storage is not the API

Paths like `tables/messages/zzz.ten` are important, but users should not have
to know them during normal application work.

Storage details belong to `tensack-store` and file-format docs.

### Honest status

Do not claim a feature is done because it exists in a spec. Mark it implemented
only when code and tests support it.

### No cute public names

Use obvious names:

- `Value` for row values
- `get` for current state once
- `watch` for live subscriptions once implemented
- `write` for applying declared changes
- `schema`, `format`, `store`, `runtime`, `cli`

Names should explain the boundary.

### Small boundaries

Each package should own one idea. If a package starts becoming “miscellaneous,”
rename or split it.

### Boring durability

Canonical data should be readable enough to inspect locally. Generated indexes
and caches must be rebuildable.

## Non-goals

- No SQL-shaped public query language.
- No hosted database dependency as the main engine.
- No interactive shell until the runtime contract is stable.
- No exposing cache or chunk internals as normal user API.
- No broad speculative abstraction before a narrow behavior needs it.
