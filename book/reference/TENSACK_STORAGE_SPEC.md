# Tensack Storage Spec

Tensack storage is local and directory-backed. The primary engine must not be a
hosted database or SQL database.

## Source Of Truth

```txt
schema.tensack + tables/**/*.ten are truth.
```

Generated or operational state:

```txt
tensack.toml
engine/*.tenb
engine/*.tenx
```

`.tenb` and `.tenx` must be rebuildable from schema plus `.ten` data.

## Directory Shape

```txt
my-chat.tensack/
  schema.tensack
  tensack.toml
  tables/
    messages/
      zz/
        zzz.ten
        zzy.ten
    users/
      zz/
        zzz.ten
  engine/
    messages.tenb
    users.tenb
```

## Chunk Naming

Chunk paths use the rule from
[tensack_chunk_naming_spec.md](tensack_chunk_naming_spec.md):

```txt
<generation>/<chunk>.ten
```

Where:

```txt
generation width = 2
chunk width      = 3
alphabet         = 0123456789abcdefghijklmnopqrstuvwxyz
```

Examples:

```txt
counter 0      -> zz/zzz.ten
counter 1      -> zz/zzy.ten
counter 2      -> zz/zzx.ten
counter 46655  -> zz/000.ten
counter 46656  -> zy/zzz.ten
counter 93312  -> zx/zzz.ten
```

The internal counter only increases. The visible names count backward so newer
chunks sort above older chunks in normal ascending folder views.

## `.ten` Row Chunks

`.ten` files are readable row chunks.

Current profile:

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

- `R` is a full replacement row.
- `D` is a delete tombstone by id.
- Values are written in schema field order.
- Tabs, newlines, carriage returns, and backslashes are escaped.
- Broken final lines may be recoverable later, but recovery is not a stable
  public feature yet.

## `tensack.toml`

`tensack.toml` is readable operational metadata, not a second schema.

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

## `.tenb`

`.tenb` is generated lookup/cache state.

Current implementation uses a binary-packed v2 encoding that stores:

- version
- table name
- schema hash
- source hash for `.ten` chunks
- live row id to row pointer entries
- lookup field/key to row id entries

The `.tenb` encoding remains internal and disposable. The runtime can decode
legacy text v1 caches for migration, but stale or legacy caches are rebuilt from
canonical `.ten` source.

## `.tenx`

`.tenx` is reserved for optional generated full-text search. It is not required
for id lookup, declared lookup reads, or normal CRUD.

## Current Implementation

Implemented today in `packages/tensack-store`:

- directory creation
- `tensack.toml`
- reverse-sorted chunk paths
- append-only `.ten` writes
- `.tenb` rebuilds
- id lookup through `.tenb`
- declared lookup reads through `.tenb`
- table scans and counts through `.tenb`

Not implemented yet:

- compaction
- repair CLI
- `.tenx`
- durable cursor storage
