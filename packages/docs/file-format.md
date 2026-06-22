# sixpack File Layout

This document describes the durable layout used by the current local store:
the internal [book storage chapter](../../book/05-storage.md) is the current
storage decision spec.

```txt
schema.sixpack  = logical schema truth
*.6           = canonical readable table row segments
*.6b          = generated cache for row pointers and lookup indexes
*.6x          = optional generated full-text search index
sixpack.toml    = readable physical layout and recoverable engine state map
```

The important rule:

```txt
schema.sixpack + tables/**/*.6 are truth.
sixpack.toml, *.6b, and *.6x are operational/generated state.
*.6b and *.6x files must be rebuildable from schema.sixpack and .6 data.
```

## Database Directory Shape

```txt
my-chat.sixpack/
  schema.sixpack
  sixpack.toml
  tables/
    users/
      zzz.6
      zzy.6
    messages/
      zzz.6
  engine/
    users.6b
    messages.6b
    messages.6x
```

`.6x` is optional and should only exist once full-text search is implemented
for a table.

## Readable Row Files

`.6` files are sixpack-readable row segments. The primary product surface is
`get` selectors, future `watch` subscriptions, and `write` changes generated
from generic tables with primitive fields. The current table profile uses:

```txt
SIX<TAB>1<TAB>table<TAB>messages<TAB><schema_hash>
@field<TAB>id<TAB>id
@field<TAB>body<TAB>text
@lookup<TAB>id<TAB>unique
@data
R<TAB>1<TAB>m1<TAB>hello
D<TAB>2<TAB>m1
```

Rules:

- `.6` is canonical source data.
- rows belong to normal schema tables.
- `R` appends a full replacement row.
- `D` appends a delete tombstone by id.
- writers append to the current segment until the engine rolls to a new chunk.
- chunk paths use flat reverse lowercase base-36 names: `<chunk>.6`.
- chunk filenames are 3 characters and live directly under each table directory.
- the first chunks are `zzz.6`, `zzy.6`, `zzx.6`.
- generation folders are not part of the current runtime layout.
- broken final lines can be ignored during recovery later.

Tabs and newlines inside values are escaped:

```txt
\  -> \\
tab -> \t
newline -> \n
carriage return -> \r
```

## Generated Cache

`.6b` is a generated cache, not source data. The current implementation uses
a binary-packed v2 encoding that stores:

- SIXB version
- table name
- schema hash
- source hash for all `.6` chunks
- live row id to row pointer entries
- lookup field/key to row id entries

The runtime rebuilds `.6b` when it is missing, stale, corrupt, or built for a
different schema/source hash. Hot writes may publish the current `.6b`
projection in memory without immediately rewriting the generated `.6b` file.
Fresh handles hash canonical `.6` data lines to reject stale cache bytes and
rebuild from `.6`. Normal id and lookup reads use `.6b`, then seek back into
the canonical `.6` row segment. Legacy text v1 caches can be decoded for
migration, but they are treated as stale and rebuilt as binary v2 caches.

## Search Index

`.6x` is reserved for optional full-text search. Exact id lookup, declared
metadata lookup, and normal reads should use `.6b`. Missing `.6x` files must
not affect normal reads.

## Metadata File

Use one root metadata file named `sixpack.toml`.

`sixpack.toml` is not a second schema. It is the readable map of physical files
and recoverable engine state. It should stay small and should not contain
per-row or per-key index data. Hot writes may leave counters behind the newest
`.6` rows; fresh handles recover from canonical `.6` data.

Example:

```toml
version = 1
schema_hash = "abc123"
next_tx = 3

[tables.messages]
id = 1
path = "tables/messages"
next_chunk = 2
header = "id\tbody"

[tables.messages.index]
state = "ready"
file = "engine/messages.6b"
source_hash = "..."
```

## Runtime Scope

Implemented now:

- `.6` magic/directive preamble for table row segments
- append-only `R` put rows and `D` delete tombstones
- generated `.6b` cache rebuilds from `.6`
- id lookup through `.6b`
- declared lookup selectors through `.6b`
- table scan and count through `.6b`
- `sixpack.toml` physical layout metadata
- batch writes that append multiple `R`/`D` operations to one `.6` chunk

Not implemented yet:

- segment sealing/compaction
- repair CLI
- `.6x` full-text search
