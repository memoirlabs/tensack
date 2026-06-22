# Storage

sixpack storage is local and directory-backed.

The source of truth is:

```txt
schema.sixpack + tables/**/*.6
```

Generated state:

```txt
sixpack.toml
engine/*.6b
engine/*.6x
```

Generated state must be rebuildable.

The target direction is still a directory database with readable table folders,
but generated binary engine state should collapse behind one private pack file:

```txt
engine/state.6pack
```

That pack is not primary data. It is an engine-owned rebuildable speed layer.
The active decision record is
[decisions/0001-generated-engine-state-pack.md](decisions/0001-generated-engine-state-pack.md).

## Directory Shape

Current implementation:

```txt
my-chat.sixpack/
  schema.sixpack
  sixpack.toml
  tables/
    messages/
      zzz.6
      zzy.6
    users/
      zzz.6
  engine/
    messages.6b
    users.6b
```

Target direction:

```txt
my-chat.sixpack/
  schema.sixpack
  sixpack.toml
  tables/
    messages/
      zzz.6
      zzy.6
    users/
      zzz.6
  engine/
    state.6pack
```

## Chunk Naming

Chunk paths:

```txt
<chunk>.6
```

Rules:

```txt
alphabet         = 0123456789abcdefghijklmnopqrstuvwxyz
chunk width      = 3
```

Examples:

```txt
counter 0      -> zzz.6
counter 1      -> zzy.6
counter 2      -> zzx.6
counter 46655  -> 000.6
```

The counter increases normally. The visible name counts backward so newer files
sort above older files in normal folder views.

Chunk files are intentionally flat under each table directory in the current
runtime. Do not add generation folders unless the product explicitly needs that
extra scale later.

The long original naming note is archived at
[reference/sixpack_chunk_naming_spec.md](reference/sixpack_chunk_naming_spec.md).

## Metadata

`sixpack.toml` is operational state, not a second schema.

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

## Store Boundary

Storage behavior belongs in:

```txt
packages/sixpack-store
```

The store owns:

- directory creation
- chunk paths
- validated write batches
- append-only `.6` chunk writes
- scans over `.6`
- `.6b` rebuilds
- row pointer reads
- lookup/cache operations
- the boundary that decides whether generated state is stored as per-table
  `.6b` files today or a single `state.6pack` later

Normal writes keep metadata compact and recoverable. Chunk lists are derived by
scanning the table directory when needed; they are not rewritten into
`sixpack.toml` on every append. Hot one-row writes may leave metadata counters
behind the newest `.6` data. Fresh handles recover `next_tx` from `.6`
operation rows.

Hot writes append to the current `.6` segment until the store rolls to a new
chunk. They may publish the generated `.6b` projection in memory and leave the
on-disk `.6b` file stale; later database handles hash canonical `.6` data
lines to detect stale `.6b` bytes and rebuild generated state.

As the engine moves to `engine/state.6pack`, this invariant stays the same:
generated binary state may lag, but canonical `.6` data must be enough to
recover it.
