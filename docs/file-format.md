# Tensack File Layout

This document describes the durable layout used by the current local store:

```txt
schema.tensack  = logical schema truth
*.ten           = readable table row segments, TSV-style
tensack.toml    = readable physical layout and engine state map
*.btf           = private binary Tensack files for indexes/lookups/cache
```

The important rule:

```txt
schema.tensack + tables/**/*.ten are truth.
tensack.toml and *.btf files are operational state.
*.btf files must be rebuildable from schema.tensack and .ten data.
```

## Workspace Shape

Recommended layout:

```txt
my-chat.tensack/
  schema.tensack
  tensack.toml
  tables/
    users/
      active.ten
      0000.ten
      0001.ten
    conversations/
      active.ten
    messages/
      active.ten
      0000.ten
  engine/
    index.btf
    cache.btf
```

## Readable Row Files

`.ten` files are Tensack-readable row segments. They are TSV-style text:

- one row per line
- first line is the exact file header
- columns are tab-separated
- `id` is always the first user field
- only user/schema fields are stored in `.ten`
- transactions, operations, logs, sync state, and lookup state are internal
- active writes append to `active.ten`
- sealed segments use sortable four-digit names like `0000.ten`
- a table folder should not contain more than 10,000 sealed segment files

Example:

```txt
id	email	name	created_at
u1	a@example.com	Ada	1710000000
u2	b@example.com	Ben	1710000001
```

Tabs and newlines inside values should be escaped so row splitting stays cheap:

```txt
\  -> \\
tab -> \t
newline -> \n
carriage return -> \r
```

## Metadata File

Use one root metadata file named `tensack.toml`.

`tensack.toml` is not a second schema. It is the readable map of physical files
and engine state. It should stay small and should not contain per-row or per-key
index data.

TOML is the recommended metadata format because it is easy to read, has clear
sections for tables, and is fast enough when loaded once on database open. Hot
lookup/index data belongs in `.btf`, not in TOML.

Example:

```toml
version = 1
schema_hash = "abc123"
next_tx = 12500001

[tables.users]
id = 1
path = "tables/users"
active = "active.ten"
segments = ["0000.ten", "0001.ten"]
header = "id\temail\tname\tcreated_at"

[tables.users.index]
state = "ready"
file = "engine/users.btf"

[tables.messages]
id = 2
path = "tables/messages"
active = "active.ten"
segments = ["0000.ten"]
header = "id\tconversation_id\tsender_id\tbody\tcreated_at"

[tables.messages.index]
state = "ready"
file = "engine/messages.btf"
```

The `header` value is physical layout metadata. It is not schema truth. It is the
exact first line expected in that table's `.ten` files so the engine can quickly
verify segment layout.

## Binary Tensack Files

Use `.btf` for private binary Tensack files.

`.btf` files may contain:

- transaction logs
- operation logs
- sync state
- lookup indexes
- row offset maps
- segment offset maps
- cache pages
- compact index snapshots
- binary acceleration structures

Users should not need to read `.btf` files. If they are missing or corrupt, the
engine should be able to rebuild them from:

```txt
schema.tensack
tensack.toml
tables/**/*.ten
```

## Lifecycle

`tensack init` creates the small visible shell:

```txt
my-chat.tensack/
  schema.tensack
```

The first write to a table creates its folder and `active.ten` with the correct
header. `tensack.toml` is created or updated when the engine needs to record
layout state.

When `active.ten` grows large:

```txt
active.ten -> 0000.ten
new active.ten
```

`tensack.toml` records the active segment and sealed segment list. `.btf` files
handle fast reads and lookups.

If a table needs more than 10,000 sealed segments, start a new segment group
folder instead of making wider filenames. The last file in a group is
`9999.ten`.

```txt
tables/
  messages/
    g0001/
      active.ten
      0000.ten
      9999.ten
    g0002/
      active.ten
      0000.ten
```

This keeps each folder small and the names easy to scan.

## Current Runtime Scope

The current store writes `.ten` row segments, updates `tensack.toml`, and creates
placeholder `.btf` files. Real binary lookup contents and lookup-backed reads are
still future implementation work.
