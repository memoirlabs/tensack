# Write Engine

This chapter is the implementation outline for keeping the write path fast,
small, and easy to reason about.

## Invariant

```txt
.6  = canonical append-only operation log
.6b = generated immutable projection/index
store = validate batch -> append batch -> publish projection
```

There should be one mental model for all mutations. A single-row write is a
one-operation batch. Bulk insert, patch, upsert, and remove are larger batches.

## Current Target

The hot path should be:

```txt
schema-generated change(s)
  -> plan envelope(s)
  -> resolved write batch
  -> store write batch
  -> append to current .6 segment
  -> one in-memory .6b publish
  -> cached counter update
```

The store should only understand storage-ready operations:

```txt
Put(full row)
Delete(table, id)
```

It should not know patch semantics, generated API naming, CLI syntax, or admin UI
actions. Those belong above the store boundary.

## Batch Rules

- A write batch belongs to exactly one table.
- A batch has a conflict mode:
  - `InsertOnly`: each put must create a new live id.
  - `Upsert`: puts replace live rows and deletes tombstone ids.
- Validation happens before touching disk.
- The in-memory `.6b` projection is simulated before the `.6` chunk is
  written.
- If validation fails, no chunk, cache, or metadata update should be published.

## Metadata Rule

`sixpack.toml` is operational state, not an operation log and not an index.

Hot writes should keep only small counters and pointers:

```toml
next_tx = 421

[tables.users]
next_chunk = 17
```

Chunk lists, migration traces, generated SDK artifacts, and debug dumps are
derived/internal artifacts. Normal users should see the current schema and use
the public `get` / `write` surface unless they intentionally inspect internals.

The generated `.6b` file and `sixpack.toml` are allowed to lag the hot write
path because neither is canonical. Metadata can carry the latest known counters
and source hash, but a later fresh handle must recover `next_tx` from `.6`,
reject stale `.6b` bytes by hashing `.6` data lines, and rebuild generated
state when needed.

## Runtime Snapshot Direction

The generated `.6b` file remains disposable. The runtime should treat a loaded
or newly-built cache as an immutable snapshot:

```txt
Arc<SixbCache>
```

The next optimization step is a runtime-only wrapper:

```txt
SixbSnapshot {
  cache: Arc<SixbCache>,
  id index,
  lookup ranges,
}
```

That wrapper should improve repeated reads without changing the on-disk `.6b`
format or exposing storage internals.

## Resolution Direction

Patch and remove operations need current rows before appending replacements or
tombstones. The clean high-performance shape is:

```txt
collect touched ids
lookup row pointers in one snapshot
group pointers by chunk
read each chunk once
resolve rows
apply changes
append one batch
```

This keeps patch/remove fast without adding SQL, a page engine, or a separate
canonical log.

## Do Not Add Yet

- SQL parser or SQL VM.
- Hosted database dependency.
- General page engine.
- Full MVCC.
- User-visible migration logs as normal workflow.
- A second canonical format such as JSONL.

Those may be useful in other systems, but they are not the next clean step for
sixpack.
