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
- Tensack `get` by id selector vs SQLite `SELECT ... WHERE id = ?`
- Tensack `write(edit)` vs SQLite `UPDATE`
- Tensack `write(remove)` vs SQLite `DELETE`

Each sample uses a temporary directory-backed database. The benchmark measures
the current Tensack storage path, including append writes, metadata updates, and
generated `.tenb` cache maintenance after writes.
