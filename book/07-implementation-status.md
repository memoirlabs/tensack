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
- append create
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
- `db.get(selector)` for current state once
- `db.write(change)` for declared state changes
- `execute_plan`

### Schema Compiler

- parses `schema!`
- validates schema
- emits table handles
- emits generated `by` selectors
- emits generated `add`/`set`/`edit`/`remove` changes
- emits patch builders
- emits unique lookup keys
- emits page/count selectors

### CLI

- help
- version

## Not Implemented

- stable generated API snapshots
- CLI commands beyond help/version
- admin UI
- `db.watch(selector)` live subscriptions
- plan JSON serde
- repair/inspect CLI
- compaction
- `.tenx`
- durable cursor format
