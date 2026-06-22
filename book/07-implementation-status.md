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

- `.6` preambles
- `.6` operation rows
- put rows
- delete tombstones
- binary `.6b` v2 cache encoding/decoding
- legacy text `.6b` decode for rebuild migration
- target `engine/state.6pack` pack documented, not implemented

### Store

- local database directory layout
- reverse-sorted chunk naming
- append into reusable `.6` segments
- append full replacement
- same-table write batches
- delete tombstones
- compact recoverable metadata counters
- lazy disk `.6b` persistence with source-hash rebuild checks
- `next_tx` recovery from canonical `.6` operation rows
- `.6b` rebuilds
- id lookup
- declared lookup reads
- live table scans
- live counts
- unique lookup conflict checks

### Runtime

- `Database`
- `db.get(selector)` for current state once
- `db.write(change)` for declared state changes
- `db.write_many(changes)` for same-table batched changes
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
- `.6x`
- durable cursor format
- single generated `engine/state.6pack` file replacing per-table `.6b` files
