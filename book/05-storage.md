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

Chunk paths:

```txt
<generation>/<chunk>.ten
```

Rules:

```txt
alphabet         = 0123456789abcdefghijklmnopqrstuvwxyz
generation width = 2
chunk width      = 3
```

Examples:

```txt
counter 0      -> zz/zzz.ten
counter 1      -> zz/zzy.ten
counter 2      -> zz/zzx.ten
counter 46655  -> zz/000.ten
counter 46656  -> zy/zzz.ten
```

The counter increases normally. The visible name counts backward so newer files
sort above older files in normal folder views.

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
chunks = ["zz/zzz.ten", "zz/zzy.ten"]
header = "id\tbody"

[tables.messages.index]
state = "ready"
file = "engine/messages.tenb"
```

## Store Boundary

Storage behavior belongs in:

```txt
packages/tensack-store
```

The store owns:

- directory creation
- chunk paths
- append writes
- scans over `.ten`
- `.tenb` rebuilds
- row pointer reads
- lookup/cache operations
