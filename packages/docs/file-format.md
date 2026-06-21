# Tensack File Layout

This document describes the durable layout used by the current local store:
the internal [book storage chapter](../../book/05-storage.md) is the current
storage decision spec.

```txt
schema.tensack  = logical schema truth
*.ten           = canonical readable table row segments
*.tenb          = generated cache for row pointers and lookup indexes
*.tenx          = optional generated full-text search index
tensack.toml    = readable physical layout and engine state map
```

The important rule:

```txt
schema.tensack + tables/**/*.ten are truth.
tensack.toml, *.tenb, and *.tenx are operational/generated state.
*.tenb and *.tenx files must be rebuildable from schema.tensack and .ten data.
```

## Workspace Shape

```txt
my-chat.tensack/
  schema.tensack
  tensack.toml
  tables/
    users/
      zz/
        zzz.ten
        zzy.ten
    messages/
      zz/
        zzz.ten
  engine/
    users.tenb
    messages.tenb
    messages.tenx
```

`.tenx` is optional and should only exist once full-text search is implemented
for a table.

## Readable Row Files

`.ten` files are Tensack-readable row segments. The primary product surface is
`get` selectors, future `watch` subscriptions, and `write` changes generated
from generic tables with primitive fields. The current table profile uses:

```txt
TEN<TAB>1<TAB>table<TAB>messages<TAB><schema_hash>
@field<TAB>id<TAB>id
@field<TAB>body<TAB>text
@lookup<TAB>id<TAB>unique
@data
R<TAB>1<TAB>m1<TAB>hello
D<TAB>2<TAB>m1
```

Rules:

- `.ten` is canonical source data.
- rows belong to normal schema tables.
- `R` appends a full replacement row.
- `D` appends a delete tombstone by id.
- chunk paths use reverse lowercase base-36 names: `<generation>/<chunk>.ten`.
- generation folders are 2 characters and chunk filenames are 3 characters.
- the first chunks are `zz/zzz.ten`, `zz/zzy.ten`, `zz/zzx.ten`.
- broken final lines can be ignored during recovery later.

Tabs and newlines inside values are escaped:

```txt
\  -> \\
tab -> \t
newline -> \n
carriage return -> \r
```

## Generated Cache

`.tenb` is a generated cache, not source data. The current implementation uses
a binary-packed v2 encoding that stores:

- TENB version
- table name
- schema hash
- source hash for all `.ten` chunks
- live row id to row pointer entries
- lookup field/key to row id entries

The runtime rebuilds `.tenb` when it is missing, stale, corrupt, or built for a
different schema/source hash. Normal id and lookup reads use `.tenb`, then seek
back into the canonical `.ten` row segment. Legacy text v1 caches can be decoded
for migration, but they are treated as stale and rebuilt as binary v2 caches.

## Search Index

`.tenx` is reserved for optional full-text search. Exact id lookup, declared
metadata lookup, and normal reads should use `.tenb`. Missing `.tenx` files must
not affect normal reads.

## Metadata File

Use one root metadata file named `tensack.toml`.

`tensack.toml` is not a second schema. It is the readable map of physical files
and engine state. It should stay small and should not contain per-row or per-key
index data.

Example:

```toml
version = 1
schema_hash = "abc123"
next_tx = 3

[tables.messages]
id = 1
path = "tables/messages"
next_chunk = 2
chunks = ["zz/zzz.ten", "zz/zzy.ten"]
header = "id\tbody"

[tables.messages.index]
state = "ready"
file = "engine/messages.tenb"
```

## Current Runtime Scope

Implemented now:

- `.ten` magic/directive preamble for table row segments
- append-only `R` put rows and `D` delete tombstones
- generated `.tenb` cache rebuilds from `.ten`
- id lookup through `.tenb`
- declared lookup selectors through `.tenb`
- table scan and count through `.tenb`
- `tensack.toml` physical layout metadata

Not implemented yet:

- segment sealing/compaction
- repair CLI
- `.tenx` full-text search
