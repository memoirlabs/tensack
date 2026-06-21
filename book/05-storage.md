# Storage

Tensack storage is local and directory-backed.

The source of truth is:

```txt
schema.tensack + tables/**/*.ten
```

Generated state:

```txt
tensack.toml
engine/*.tenb
engine/*.tenx
```

Generated state must be rebuildable.

## Directory Shape

```txt
my-chat.tensack/
  schema.tensack
  tensack.toml
  tables/
    messages/
      zzz.ten
      zzy.ten
    users/
      zzz.ten
  engine/
    messages.tenb
    users.tenb
```

## Chunk Naming

Chunk paths:

```txt
<chunk>.ten
```

Rules:

```txt
alphabet         = 0123456789abcdefghijklmnopqrstuvwxyz
chunk width      = 3
```

Examples:

```txt
counter 0      -> zzz.ten
counter 1      -> zzy.ten
counter 2      -> zzx.ten
counter 46655  -> 000.ten
```

The counter increases normally. The visible name counts backward so newer files
sort above older files in normal folder views.

Chunk files are intentionally flat under each table directory in the current
runtime. Do not add generation folders unless the product explicitly needs that
extra scale later.

The long original naming note is archived at
[reference/tensack_chunk_naming_spec.md](reference/tensack_chunk_naming_spec.md).

## Metadata

`tensack.toml` is operational state, not a second schema.

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
file = "engine/messages.tenb"
source_hash = "..."
```

## Store Boundary

Storage behavior belongs in:

```txt
packages/tensack-store
```

The store owns:

- directory creation
- chunk paths
- validated write batches
- append-only `.ten` chunk writes
- scans over `.ten`
- `.tenb` rebuilds
- row pointer reads
- lookup/cache operations

Normal writes keep metadata compact and recoverable. Chunk lists are derived by
scanning the table directory when needed; they are not rewritten into
`tensack.toml` on every append. Hot one-row writes may leave metadata counters
behind the newest `.ten` data. Fresh handles recover `next_tx` from `.ten`
operation rows.

Hot writes append to the current `.ten` segment until the store rolls to a new
chunk. They may publish the generated `.tenb` projection in memory and leave the
on-disk `.tenb` file stale; later database handles hash canonical `.ten` data
lines to detect stale `.tenb` bytes and rebuild generated state.
