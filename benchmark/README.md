# Benchmarks

This package contains local performance comparisons for Tensack behavior that
exists today. Keep benchmark code separate from product runtime code.

## State API vs SQLite

Run from the repository root:

```sh
cargo bench -p tensack-benchmark --bench crud_vs_sqlite
```

The current benchmark compares basic state access/change behavior for a small
`users` table:

- Tensack `write(add)` vs SQLite `INSERT`
- Tensack `insert_many` vs SQLite transaction insert
- Tensack `get` by id selector vs SQLite `SELECT ... WHERE id = ?`
- Tensack binary projection `count` vs SQLite `COUNT(*)`
- Tensack `write(edit)` vs SQLite `UPDATE`
- Tensack `write_many(edit)` vs SQLite `UPDATE`
- Tensack `write(remove)` vs SQLite `DELETE`
- Tensack `write_many(remove)` vs SQLite `DELETE`

Each sample uses a temporary directory-backed database. The benchmark measures
the current Tensack storage path, including append writes, recoverable metadata,
and generated `.tenb` projection maintenance after writes.

The key engine comparison is one-row-at-a-time writes versus same-table
write batches. Batches should append to one `.ten` segment and publish one
`.tenb` projection. Metadata is recoverable and should not dominate the hot
write path.
