# Benchmarks

This package contains local performance comparisons for sixpack behavior that
exists today. Keep benchmark code separate from product runtime code.

## State API vs SQLite

Run from the repository root:

```sh
cargo bench -p sixpack-benchmark --bench crud_vs_sqlite
```

The current benchmark compares basic state access/change behavior for a small
`users` table:

- sixpack `write(add)` vs SQLite `INSERT`
- sixpack `insert_many` vs SQLite transaction insert
- sixpack `get` by id selector vs SQLite `SELECT ... WHERE id = ?`
- sixpack binary projection `count` vs SQLite `COUNT(*)`
- sixpack `write(edit)` vs SQLite `UPDATE`
- sixpack `write_many(edit)` vs SQLite `UPDATE`
- sixpack `write(remove)` vs SQLite `DELETE`
- sixpack `write_many(remove)` vs SQLite `DELETE`

Each sample uses a temporary directory-backed database. The benchmark measures
the current sixpack storage path, including append writes, recoverable metadata,
and generated `.6b` projection maintenance after writes.

The key engine comparison is one-row-at-a-time writes versus same-table
write batches. Batches should append to one `.6` segment and publish one
`.6b` projection. Metadata is recoverable and should not dominate the hot
write path.

## Hot Path vs SQLite

Run from the repository root:

```sh
RUSTFLAGS='-C target-cpu=native' \
  cargo bench -p sixpack-benchmark --bench hot_path -- \
  --sample-size 10 --warm-up-time 0.2 --measurement-time 1.0
```

This benchmark is the more realistic local-application check. It preloads
10,000 rows once, keeps the same live database handle open, then measures
1,000 operations per iteration. Read/count cases stay fixed-size. Write cases
keep mutating the same live handle, so they measure ongoing append/update
behavior instead of database regeneration.

The current benchmark groups are:

- sixpack `get` by id selector vs SQLite indexed `SELECT`
- sixpack binary projection `count` vs SQLite `COUNT(*)`
- sixpack `write(add)` vs SQLite `INSERT`
- sixpack `insert_many` vs SQLite transaction insert
- sixpack `write(edit)` vs SQLite `UPDATE`
- sixpack `write_many(edit)` vs SQLite transaction update

The intended storage comparison is:

- `.6` remains the canonical readable append source.
- `.6b` remains the generated binary lookup/count/read projection.
- hot reads should use the runtime `.6b` map and in-memory row cache.
- hot writes should update the runtime map and materialize compact `.6b`
  lazily, not rebuild the database or rewrite the binary snapshot per row.
- measured loops should not regenerate the database or rebuild `.6b`.
