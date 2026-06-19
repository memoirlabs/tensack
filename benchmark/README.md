# Benchmarks

This package contains local performance comparisons for Tensack behavior that
exists today. Keep benchmark code separate from product runtime code.

## CRUD vs SQLite

Run from the repository root:

```sh
cargo bench -p tensack-benchmark --bench crud_vs_sqlite
```

The current benchmark compares basic row CRUD for a small `users` table:

- Tensack `insert` vs SQLite `INSERT`
- Tensack `get` by id vs SQLite `SELECT ... WHERE id = ?`
- Tensack `patch_by_id` vs SQLite `UPDATE`
- Tensack `delete_by_id` vs SQLite `DELETE`

Each sample uses a temporary directory-backed database. The benchmark measures
the current Tensack storage path, including append writes, metadata updates, and
generated `.tenb` cache rebuilds after writes.
