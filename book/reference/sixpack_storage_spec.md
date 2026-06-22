# sixpack Storage Spec

sixpack storage is local and directory-backed. The primary engine must not be a
hosted database or SQL database.

## Source Of Truth

```txt
schema.sixpack + tables/**/*.6 are truth.
```

Generated or operational state:

```txt
sixpack.toml
engine/*.6b
engine/*.6x
```

`.6b` and `.6x` must be rebuildable from schema plus `.6` data.

## Directory Shape

```txt
my-chat.sixpack/
  schema.sixpack
  sixpack.toml
  tables/
    messages/
      zz/
        zzz.6
        zzy.6
    users/
      zz/
        zzz.6
  engine/
    messages.6b
    users.6b
```

## Chunk Naming

Chunk paths use the rule from
[sixpack_chunk_naming_spec.md](sixpack_chunk_naming_spec.md):

```txt
<generation>/<chunk>.6
```

Where:

```txt
generation width = 2
chunk width      = 3
alphabet         = 0123456789abcdefghijklmnopqrstuvwxyz
```

Examples:

```txt
counter 0      -> zz/zzz.6
counter 1      -> zz/zzy.6
counter 2      -> zz/zzx.6
counter 46655  -> zz/000.6
counter 46656  -> zy/zzz.6
counter 93312  -> zx/zzz.6
```

The internal counter only increases. The visible names count backward so newer
chunks sort above older chunks in normal ascending folder views.

## `.6` Row Chunks

`.6` files are readable row chunks.

Current profile:

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

- `R` is a full replacement row.
- `D` is a delete tombstone by id.
- Values are written in schema field order.
- Tabs, newlines, carriage returns, and backslashes are escaped.
- Broken final lines may be recoverable later, but recovery is not a stable
  public feature yet.

## `sixpack.toml`

`sixpack.toml` is readable operational metadata, not a second schema.

Example:

```toml
version = 1
schema_hash = "abc123"
next_tx = 3

[tables.messages]
id = 1
path = "tables/messages"
next_chunk = 2
chunks = ["zz/zzz.6", "zz/zzy.6"]
header = "id\tbody"

[tables.messages.index]
state = "ready"
file = "engine/messages.6b"
```

## `.6b`

`.6b` is generated lookup/cache state.

Current implementation uses a binary-packed v2 encoding that stores:

- version
- table name
- schema hash
- source hash for `.6` chunks
- live row id to row pointer entries
- lookup field/key to row id entries

The `.6b` encoding remains internal and disposable. The runtime can decode
legacy text v1 caches for migration, but stale or legacy caches are rebuilt from
canonical `.6` source.

## `.6x`

`.6x` is reserved for optional generated full-text search. It is not required
for id lookup, declared lookup reads, or normal CRUD.

## Current Implementation

Implemented today in `packages/sixpack-store`:

- directory creation
- `sixpack.toml`
- reverse-sorted chunk paths
- append-only `.6` writes
- `.6b` rebuilds
- id lookup through `.6b`
- declared lookup reads through `.6b`
- table scans and counts through `.6b`

Not implemented yet:

- compaction
- repair CLI
- `.6x`
- durable cursor storage
