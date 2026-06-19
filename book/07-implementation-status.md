# Implementation Status

This chapter is the honesty check.

## Implemented

### Core Model

- `DatabaseSchema`
- `TableSchema`
- `FieldSpec`
- `LookupSpec`
- `Record`
- `Value`
- `PrimitiveType`
- schema validation

### Format

- `.ten` preambles
- `.ten` operation rows
- put rows
- delete tombstones
- binary `.tenb` v2 cache encoding/decoding
- legacy text `.tenb` decode for rebuild migration

### Store

- local database directory layout
- reverse-sorted chunk naming
- append insert
- append full replacement
- delete tombstones
- `.tenb` rebuilds
- id lookup
- declared lookup reads
- live table scans
- live counts
- unique lookup conflict checks

### Runtime

- `TensackDatabase`
- `insert`
- `upsert`
- `put` compatibility alias
- `patch_by_id`
- `delete_by_id`
- `get`
- `get_by`
- `get_many_by`
- `scan`
- `count`
- `execute_plan`

### Schema Compiler

- parses `schema!`
- validates schema
- emits table handles
- emits patch builders
- emits unique lookup keys
- emits get/find/scan/count helpers

### CLI

- help
- version

## Not Implemented

- stable generated API snapshots
- CLI commands beyond help/version
- admin UI
- plan JSON serde
- repair/inspect CLI
- compaction
- `.tenx`
- durable cursor format

## Current Compatibility Names

The runtime still has:

```txt
put
delete_by_id
get_by
get_many_by
```

The target generated API should prefer:

```txt
upsert
remove
get.<lookup>
find.<lookup>
```

